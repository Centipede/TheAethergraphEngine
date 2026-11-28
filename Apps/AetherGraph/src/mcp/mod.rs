//! MCP (Model Context Protocol) server, so agents can drive AetherGraph.
//!
//! Mounted at `/mcp` on the main server (see [`crate::routes::build_router`]) using
//! rmcp's Streamable HTTP transport, which is a `tower::Service` and therefore nests
//! straight into the existing Axum router. That keeps it in one process, sharing
//! [`AppState`] with the REST handlers instead of duplicating storage logic.
//!
//! Adding a tool means adding one `#[tool]` fn to the `impl AetherGraphMcp` block below.
//!
//! Everything here is read-only except `set_focus`/`release_focus`, which touch in-memory
//! session state and never a book. Write tools are a deliberate follow-up: they are only
//! worth building once the perception tools are shown to read a page correctly.

pub mod dto;
pub mod features;
pub mod pages;

use base64::Engine as _;
use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::wrapper::{Json, Parameters},
    model::*,
    schemars, tool, tool_handler, tool_router,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};

use crate::hocr_parser::{self, HocrPage, HocrPath, ParserConfig};
use crate::routes::projects::models::{Page, PageDb, Project};
use crate::routes::projects::storage;
use crate::state::{AgentFocus, AppState};

/// How many pages either side of the requested one are examined when deciding whether a
/// block is a running header. Bounded so one call cannot walk a whole book.
const REPEAT_WINDOW: usize = 3;

/// Default padding, in scan pixels, around a cropped element.
const DEFAULT_PADDING: u32 = 24;

// ── TOOL PARAMETERS ──────────────────────────────────────────────────────────

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ProjectArgs {
    /// The project's `machine_name`, as returned by `list_projects`.
    pub machine_name: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct PageArgs {
    /// The project's `machine_name`.
    pub machine_name: String,
    /// Which page: either the scan stem (`prolog-5`) or the page number as printed in
    /// the book (`vi`). These are different things and often do not correspond — call
    /// `list_pages` if unsure.
    pub page: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct PageImageArgs {
    /// The project's `machine_name`.
    pub machine_name: String,
    /// Either the scan stem (`prolog-5`) or the printed page number (`vi`).
    pub page: String,
    /// Longest edge of the returned image, in pixels. Defaults to 1400 and is capped at
    /// 2000. Raise it for a dense multi-column page that will not read at the default;
    /// lower it to save tokens. It never changes the coordinates reported by `get_page`.
    pub scale: Option<u32>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct BlockImageArgs {
    /// The project's `machine_name`.
    pub machine_name: String,
    /// Either the scan stem (`prolog-5`) or the printed page number (`vi`).
    pub page: String,
    /// Ids of the elements to crop, as returned by `get_page`. A carea, block, line or
    /// word id all work. Passing several returns several crops in one result, which is
    /// how to compare the typography of, say, a known chapter heading against a
    /// candidate.
    pub ids: Vec<String>,
    /// Extra margin around each element, in scan pixels. Defaults to 24.
    pub padding: Option<u32>,
    /// Longest edge of each returned crop, in pixels. Defaults to 1400, capped at 2000.
    pub scale: Option<u32>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct FocusArgs {
    /// The project's `machine_name`.
    pub machine_name: String,
    /// Either the scan stem (`prolog-5`) or the printed page number (`vi`).
    pub page: String,
    /// Which folio mode to show: `assist`, `crop`, `hint`, `recognise`, `edit` or
    /// `bridge`. Use `edit` for working on blocks and their types.
    pub mode: String,
}

// ── HANDLER ──────────────────────────────────────────────────────────────────

/// The MCP server handler. One instance is built per session.
#[derive(Clone)]
pub struct AetherGraphMcp {
    state: AppState,
}

impl AetherGraphMcp {
    fn page_db(&self, machine_name: &str) -> PageDb {
        storage::load_page_db(&self.state.project_pagesdb_path(machine_name))
    }

    fn project(&self, machine_name: &str) -> Result<Project, McpError> {
        storage::read_project(&self.state.projects_dir, machine_name).map_err(|_| {
            McpError::resource_not_found(
                format!(
                    "No project '{machine_name}'. Call list_projects for the available \
                     machine_names."
                ),
                None,
            )
        })
    }

    /// Resolve a stem-or-printed-name query against a project's pages.
    fn resolve(
        &self,
        machine_name: &str,
        query: &str,
    ) -> Result<(pages::PageRef, Page), McpError> {
        let db = self.page_db(machine_name);
        let page_ref = pages::resolve_page(&db.pages, query)
            .map_err(|e| McpError::invalid_params(e.message(), None))?;
        let page = db
            .pages
            .into_iter()
            .find(|p| p.scan == page_ref.scan)
            .ok_or_else(|| McpError::internal_error("page vanished during resolution", None))?;
        Ok((page_ref, page))
    }

    fn has_hocr(&self, machine_name: &str, stem: &str) -> bool {
        storage::hocr_active_path(&self.state.projects_dir, machine_name, stem).is_some()
    }

    /// Read and parse one page's hOCR, with block metrics filled in.
    ///
    /// Mirrors `handlers_api::get_hocr_json`: parse, then `calculate_all_metrics` through
    /// a `FileSystemPageProvider`, because metrics depend on neighbouring pages.
    fn parse_page(&self, machine_name: &str, stem: &str) -> Result<HocrPage, McpError> {
        let path = storage::hocr_active_path(&self.state.projects_dir, machine_name, stem)
            .ok_or_else(|| {
                McpError::resource_not_found(
                    format!(
                        "Page '{stem}' has no hOCR, so it has no blocks to inspect. It has \
                         not been OCR'd yet. You can still look at it with get_page_image."
                    ),
                    None,
                )
            })?;

        let html = std::fs::read_to_string(&path)
            .map_err(|e| McpError::internal_error(format!("cannot read hOCR: {e}"), None))?;

        let mut page = hocr_parser::parse(
            &html,
            ParserConfig {
                block_metrics: true,
                line_metrics: false,
            },
        )
        .ok_or_else(|| {
            McpError::internal_error(format!("hOCR for '{stem}' could not be parsed"), None)
        })?;

        if let Some(provider) = hocr_parser::FileSystemPageProvider::new(
            self.state.projects_dir(),
            machine_name,
        ) {
            page.calculate_all_metrics(&provider, stem);
        }

        Ok(page)
    }

    /// Up to `REPEAT_WINDOW` pages either side that have hOCR, for running-header
    /// detection. Parsed without metrics, since only text and geometry are needed.
    fn neighbour_pages(&self, machine_name: &str, stem: &str) -> Vec<HocrPage> {
        let db = self.page_db(machine_name);
        let stems: Vec<String> = db
            .pages
            .iter()
            .map(|p| pages::stem_of(&p.scan))
            .collect();

        let Some(idx) = stems.iter().position(|s| s == stem) else {
            return vec![];
        };

        let lo = idx.saturating_sub(REPEAT_WINDOW);
        let hi = (idx + REPEAT_WINDOW + 1).min(stems.len());

        stems[lo..hi]
            .iter()
            .filter(|s| s.as_str() != stem)
            .filter_map(|s| {
                let path =
                    storage::hocr_active_path(&self.state.projects_dir, machine_name, s)?;
                let html = std::fs::read_to_string(&path).ok()?;
                hocr_parser::parse(&html, ParserConfig::default())
            })
            .collect()
    }

    /// The page scan with the project's crop, processing settings and hints applied, so
    /// the agent sees the same image the user is looking at.
    fn processed_scan(&self, machine_name: &str, page: &Page) -> Result<Vec<u8>, McpError> {
        let project = self.project(machine_name)?;
        let path = self
            .state
            .projects_dir
            .join(machine_name)
            .join("pages")
            .join("scans")
            .join(&page.scan);

        let bytes = std::fs::read(&path).map_err(|e| {
            McpError::internal_error(format!("cannot read scan '{}': {e}", page.scan), None)
        })?;

        crate::image_utils::apply_image_pipeline(
            &bytes,
            Some(page.crop_edges),
            project.processing.as_ref(),
            &page.hints,
        )
        .map_err(|e| McpError::internal_error(format!("image pipeline failed: {e}"), None))
    }

    fn folio_url(machine_name: &str, mode: &str, stem: &str) -> String {
        format!("/projects/{machine_name}/folios/{mode}/{stem}")
    }
}

#[tool_router]
impl AetherGraphMcp {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    #[tool(
        description = "List the book projects available in this AetherGraph instance. \
                       Returns each project's machine_name, which is the identifier \
                       used by every other tool and by the REST API."
    )]
    async fn list_projects(&self) -> Result<Json<dto::ProjectList>, McpError> {
        Ok(Json(dto::summarise(storage::read_projects(&self.state))))
    }

    #[tool(
        description = "Get one project in detail: its metadata, the text flows and \
                       layouts it declares, how many pages it has and how many of those \
                       have been OCR'd."
    )]
    async fn get_project(
        &self,
        Parameters(args): Parameters<ProjectArgs>,
    ) -> Result<Json<dto::ProjectDetail>, McpError> {
        let project = self.project(&args.machine_name)?;
        let db = self.page_db(&args.machine_name);
        let with_hocr = db
            .pages
            .iter()
            .filter(|p| self.has_hocr(&args.machine_name, &pages::stem_of(&p.scan)))
            .count();
        Ok(Json(dto::detail(project, db.pages.len(), with_hocr)))
    }

    #[tool(
        description = "List a project's pages in book order. Use this to translate a \
                       page number printed in the book (such as 'vi' or '11') into the \
                       scan stem that the other tools take. The two rarely match: front \
                       matter is often numbered in Roman numerals, and scan numbering \
                       can be offset from printed numbering. Also shows which pages have \
                       been OCR'd."
    )]
    async fn list_pages(
        &self,
        Parameters(args): Parameters<ProjectArgs>,
    ) -> Result<Json<dto::PageList>, McpError> {
        // Confirms the project exists, so an unknown machine_name does not come back as
        // a confusingly empty page list.
        self.project(&args.machine_name)?;

        let db = self.page_db(&args.machine_name);
        let pages_out = db
            .pages
            .iter()
            .map(|p| {
                let stem = pages::stem_of(&p.scan);
                dto::PageSummary {
                    has_hocr: self.has_hocr(&args.machine_name, &stem),
                    has_unsaved_edits: storage::has_unsaved_edits(
                        &self.state.projects_dir,
                        &args.machine_name,
                        &p.scan,
                    ),
                    stem,
                    name: p.name.clone(),
                    scan: p.scan.clone(),
                }
            })
            .collect();

        Ok(Json(dto::PageList {
            machine_name: args.machine_name,
            pages: pages_out,
        }))
    }

    #[tool(
        description = "Read one page's structure: its content areas, the text blocks \
                       inside them, and measurements of how each block looks. Use this \
                       to find headings and work out a page's structure. \
                       Note two things. Reading order runs down each carea in turn, not \
                       across the page by vertical position, so a multi-column page must \
                       be read column by column. And no single size measurement is \
                       reliable: when the numbers are close, confirm against the scan \
                       with get_page_image or get_block_image. All coordinates are \
                       original scan pixels."
    )]
    async fn get_page(
        &self,
        Parameters(args): Parameters<PageArgs>,
    ) -> Result<Json<dto::PageView>, McpError> {
        let (page_ref, page) = self.resolve(&args.machine_name, &args.page)?;
        let parsed = self.parse_page(&args.machine_name, &page_ref.stem)?;
        let neighbours = self.neighbour_pages(&args.machine_name, &page_ref.stem);
        let unsaved = storage::has_unsaved_edits(
            &self.state.projects_dir,
            &args.machine_name,
            &page.scan,
        );

        Ok(Json(dto::page_view(
            &args.machine_name,
            &page_ref,
            unsaved,
            &parsed,
            &neighbours,
        )))
    }

    #[tool(
        description = "Look at a whole page scan. Works whether or not the page has been \
                       OCR'd, so it is the way to read something OCR cannot handle, such \
                       as a multi-column table of contents. The image has the project's \
                       crop, processing settings and hints applied, so it matches what \
                       the user sees. The image may be downscaled; never measure \
                       coordinates off it, take them from get_page instead."
    )]
    async fn get_page_image(
        &self,
        Parameters(args): Parameters<PageImageArgs>,
    ) -> Result<CallToolResult, McpError> {
        let (page_ref, page) = self.resolve(&args.machine_name, &args.page)?;
        let processed = self.processed_scan(&args.machine_name, &page)?;
        let max_edge = args.scale.unwrap_or(pages::DEFAULT_SCALE);

        let (jpeg, ow, oh, w, h) = pages::to_jpeg(&processed, max_edge)
            .map_err(|e| McpError::internal_error(format!("cannot encode image: {e}"), None))?;

        let caption = format!(
            "{} page {} (stem {}), scan {}. Original {ow}x{oh} px, returned {w}x{h} px \
             (scale {:.3}). Coordinates from get_page are in the original {ow}x{oh} \
             pixel space, not this image's.",
            args.machine_name,
            page_ref.name,
            page_ref.stem,
            page_ref.scan,
            w as f32 / ow.max(1) as f32,
        );

        Ok(CallToolResult::success(vec![
            ContentBlock::text(caption),
            ContentBlock::image(
                base64::engine::general_purpose::STANDARD.encode(&jpeg),
                "image/jpeg",
            ),
        ]))
    }

    #[tool(
        description = "Look at specific parts of a page close up: pass the ids of \
                       careas, blocks, lines or words from get_page and get a cropped \
                       image of each. Passing several ids returns several crops in one \
                       result, which is the way to compare how two headings are set and \
                       decide whether they are the same level."
    )]
    async fn get_block_image(
        &self,
        Parameters(args): Parameters<BlockImageArgs>,
    ) -> Result<CallToolResult, McpError> {
        if args.ids.is_empty() {
            return Err(McpError::invalid_params(
                "ids is empty: name at least one carea, block, line or word id from \
                 get_page."
                    .to_string(),
                None,
            ));
        }

        let (page_ref, page) = self.resolve(&args.machine_name, &args.page)?;
        let parsed = self.parse_page(&args.machine_name, &page_ref.stem)?;
        let processed = self.processed_scan(&args.machine_name, &page)?;

        let padding = args.padding.unwrap_or(DEFAULT_PADDING);
        let max_edge = args.scale.unwrap_or(pages::DEFAULT_SCALE);

        let mut blocks = Vec::new();
        for id in &args.ids {
            let Some(path) = hocr_parser::find_node(&parsed, id) else {
                return Err(McpError::invalid_params(
                    format!(
                        "No element '{id}' on page {} (stem {}). Ids come from get_page \
                         for this same page.",
                        page_ref.name, page_ref.stem
                    ),
                    None,
                ));
            };

            let (bbox, what) = describe_node(&parsed, path).ok_or_else(|| {
                McpError::internal_error(format!("cannot locate '{id}' after lookup"), None)
            })?;

            // Empty image-block and hint lists on purpose: this function blots those
            // regions white, which is right when re-OCRing a region and wrong here,
            // where the whole point is to see what is printed.
            let cropped = crate::image_utils::extract_and_process_carea_image(
                &processed,
                bbox,
                crate::routes::projects::models::CropEdges::default(),
                &[],
                &[],
                padding,
                None,
            )
            .map_err(|e| McpError::internal_error(format!("cannot crop '{id}': {e}"), None))?;

            let (jpeg, ow, oh, w, h) = pages::to_jpeg(&cropped, max_edge).map_err(|e| {
                McpError::internal_error(format!("cannot encode crop of '{id}': {e}"), None)
            })?;

            blocks.push(ContentBlock::text(format!(
                "{id} ({what}) on page {} (stem {}), bbox [{}, {}, {}, {}] in scan \
                 pixels. Crop {ow}x{oh} px, returned {w}x{h} px.",
                page_ref.name,
                page_ref.stem,
                bbox.left(),
                bbox.top(),
                bbox.right(),
                bbox.bottom(),
            )));
            blocks.push(ContentBlock::image(
                base64::engine::general_purpose::STANDARD.encode(&jpeg),
                "image/jpeg",
            ));
        }

        Ok(CallToolResult::success(blocks))
    }

    #[tool(
        description = "Point the user's browser at a page, so they can watch along as \
                       you work. Call it before working on each page, so the view steps \
                       through the book with you. While focus is set, the interface shows \
                       the user that an agent is driving. This changes only what the user \
                       is looking at; it does not modify the book."
    )]
    async fn set_focus(
        &self,
        Parameters(args): Parameters<FocusArgs>,
    ) -> Result<Json<dto::FocusView>, McpError> {
        let mode = pages::validate_mode(&args.mode)
            .map_err(|e| McpError::invalid_params(e, None))?;
        let (page_ref, _) = self.resolve(&args.machine_name, &args.page)?;

        let mut guard = self
            .state
            .agent_focus
            .write()
            .map_err(|_| McpError::internal_error("focus lock poisoned", None))?;

        let seq = guard.as_ref().map(|f| f.seq.wrapping_add(1)).unwrap_or(1);
        *guard = Some(AgentFocus {
            machine_name: args.machine_name.clone(),
            stem: page_ref.stem.clone(),
            mode: mode.clone(),
            seq,
            updated_at: std::time::SystemTime::now(),
        });

        Ok(Json(dto::FocusView {
            url: Self::folio_url(&args.machine_name, &mode, &page_ref.stem),
            machine_name: args.machine_name,
            stem: page_ref.stem,
            name: page_ref.name,
            mode,
        }))
    }

    #[tool(
        description = "Stop steering the user's browser and clear the 'agent is driving' \
                       indicator. Call this when you have finished a run of page work."
    )]
    async fn release_focus(&self) -> Result<String, McpError> {
        let mut guard = self
            .state
            .agent_focus
            .write()
            .map_err(|_| McpError::internal_error("focus lock poisoned", None))?;
        *guard = None;
        Ok("Focus released; the user is back in control of the view.".to_string())
    }
}

/// The bbox of whatever an id resolved to, plus a word for what it is.
fn describe_node(
    page: &HocrPage,
    path: HocrPath,
) -> Option<(crate::hocr_parser::HocrBbox, &'static str)> {
    match path {
        HocrPath::Page => Some((page.bbox, "page")),
        HocrPath::Carea { carea } => Some((page.careas.get(carea)?.bbox, "carea")),
        HocrPath::Block { carea, block } => {
            Some((page.careas.get(carea)?.blocks.get(block)?.bbox, "block"))
        }
        HocrPath::Line { carea, block, line } => Some((
            page.careas.get(carea)?.blocks.get(block)?.lines.get(line)?.bbox,
            "line",
        )),
        HocrPath::Word {
            carea,
            block,
            line,
            word,
        } => Some((
            page.careas
                .get(carea)?
                .blocks
                .get(block)?
                .lines
                .get(line)?
                .words
                .get(word)?
                .bbox,
            "word",
        )),
    }
}

#[tool_handler]
impl ServerHandler for AetherGraphMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            // Not `Implementation::from_build_env()`: that expands `env!("CARGO_CRATE_NAME")`
            // inside rmcp itself, so it reports the server as "rmcp" 3.11.0 rather than us.
            .with_server_info(
                Implementation::new("aethergraph", env!("CARGO_PKG_VERSION"))
                    .with_title("AetherGraph"),
            )
            // Protocol version is left at rmcp's default. A server answering `initialize`
            // can never name 2026-07-28, since that revision replaced the handshake with
            // per-request `_meta`; rmcp negotiates 2025-11-25 there and handles the newer
            // stateless path on its own when a client asks for it.
            .with_instructions(
                "AetherGraph ingests, transcribes and indexes scanned books and \
                 documents, preserving their physical layout and structural hierarchy. \
                 Work is organised into projects, one per book or document.\n\n\
                 Three things to know before working on a book.\n\n\
                 1. Every project is identified by its machine_name; use that value, not \
                 the display name and not the abbrev, whenever a tool or an API route \
                 asks which project to act on.\n\n\
                 2. A page has two names and they usually differ. The scan stem (such as \
                 prolog-5) is the identifier the tools take. The printed page name (such \
                 as vi, or 11) is what the reader sees and what the user will say. Front \
                 matter is often numbered in Roman numerals, and scan numbering can be \
                 offset from printed numbering, so never assume page 11 is side-011. Call \
                 list_pages to get the mapping, and tell the user the printed name while \
                 passing the stem to tools.\n\n\
                 3. All coordinates are original scan pixels. Images returned by \
                 get_page_image and get_block_image may be downscaled and state their \
                 scale; take geometry from get_page, never by measuring an image.\n\n\
                 When working through pages, call set_focus first for each page so the \
                 user's screen follows along, and release_focus when you finish."
                    .to_string(),
            )
    }
}

/// Build the Streamable HTTP service to nest at `/mcp`.
///
/// The default config restricts `Host` headers to loopback, which is rmcp's built-in
/// protection against DNS rebinding attacks on locally running servers. Keep it.
pub fn service(state: AppState) -> StreamableHttpService<AetherGraphMcp, LocalSessionManager> {
    StreamableHttpService::new(
        move || Ok(AetherGraphMcp::new(state.clone())),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default(),
    )
}

#[cfg(test)]
mod tests;
