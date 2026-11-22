//! MCP (Model Context Protocol) server, so agents can drive AetherGraph.
//!
//! Mounted at `/mcp` on the main server (see [`crate::routes::build_router`]) using
//! rmcp's Streamable HTTP transport, which is a `tower::Service` and therefore nests
//! straight into the existing Axum router. That keeps it in one process, sharing
//! [`AppState`] with the REST handlers instead of duplicating storage logic.
//!
//! Adding a tool means adding one `#[tool]` fn to the `impl AetherGraphMcp` block below.

use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::wrapper::Json,
    model::*,
    schemars, tool, tool_handler, tool_router,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};

use crate::routes::projects::models::Project;
use crate::routes::projects::storage;
use crate::state::AppState;

// NOTE: these are deliberately separate types from `Project` rather than deriving
// `schemars::JsonSchema` on the domain model, which would pull schemars derives through
// `FlowSchema`, `LayoutSchema`, `EditorPalette`, `ColorSpecification` and
// `ProcessingSettings` for no benefit, since an agent listing projects wants none of
// them. If these summary structs start multiplying, reconsider.
//
// Keep `///` comments here strictly agent-facing: the `#[tool]` macro compiles them into
// the tool's output schema, so anything written here is read by the agent as
// documentation. Implementation rationale belongs in `//` comments like this one.

/// A book or document project in this AetherGraph instance.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ProjectSummary {
    /// Stable identifier for the project. Use this value wherever a `machine_name` is
    /// wanted, including the `{machine_name}` segment of `/api/projects/{machine_name}`.
    /// It is also the project's directory name on disk.
    pub machine_name: String,
    /// Human-readable title of the book or document.
    pub name: String,
    /// Short display code for the project, if one is set. This is a label, not an
    /// identifier — never use it in place of `machine_name`.
    pub abbrev: Option<String>,
    /// Free-text description, if one is set.
    pub description: Option<String>,
    /// Full names of the authors, if any are recorded.
    pub authors: Vec<String>,
    /// Publication date as `YYYY-MM-DD`, if one is set.
    pub published: Option<String>,
    /// Tesseract language code used for OCR on this project, if one is set.
    pub ocr_language: Option<String>,
}

// Wraps the list because MCP requires `structuredContent` to be a JSON object at the top
// level; a bare array would be invalid.

/// The projects available in this AetherGraph instance.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ProjectList {
    /// Every project in this AetherGraph instance, sorted by `machine_name`.
    pub projects: Vec<ProjectSummary>,
}

/// Format a date the same way the `optional_date` serde helper in
/// [`crate::routes::projects::models`] does, so MCP and the REST API agree.
fn format_date(date: time::Date) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        date.month() as u8,
        date.day()
    )
}

/// Map domain projects onto the agent-facing summaries.
///
/// Sorted by `machine_name` because [`storage::read_projects`] yields `fs::read_dir`
/// order, which is not stable across machines or runs; agents do better with
/// deterministic output.
fn summarise(projects: Vec<Project>) -> ProjectList {
    let mut projects: Vec<ProjectSummary> = projects
        .into_iter()
        .map(|p| ProjectSummary {
            machine_name: p.machine_name,
            name: p.name,
            abbrev: p.abbrev,
            description: p.description,
            authors: p.authors.into_iter().map(|a| a.full_name).collect(),
            published: p.published.map(format_date),
            ocr_language: p.ocr_language,
        })
        .collect();

    projects.sort_by(|a, b| a.machine_name.cmp(&b.machine_name));

    ProjectList { projects }
}

/// The MCP server handler. One instance is built per session.
#[derive(Clone)]
pub struct AetherGraphMcp {
    state: AppState,
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
    async fn list_projects(&self) -> Result<Json<ProjectList>, McpError> {
        Ok(Json(summarise(storage::read_projects(&self.state))))
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
                 Work is organised into projects, one per book or document. Every \
                 project is identified by its machine_name; use that value, not the \
                 display name and not the abbrev, whenever a tool or an API route asks \
                 which project to act on."
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
