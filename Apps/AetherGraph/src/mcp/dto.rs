//! The agent-facing shapes, and the mapping from domain types onto them.
//!
//! These are deliberately separate types from the domain models rather than deriving
//! `schemars::JsonSchema` on `Project`, `HocrPage` and friends. Doing that would pull
//! schemars derives through `FlowSchema`, `LayoutSchema`, `EditorPalette`,
//! `ColorSpecification`, `HocrBlockHints` and the whole `Evidence` machinery, none of
//! which an agent reading a page has any use for. It also keeps a tool's contract from
//! changing silently whenever a domain struct gains a field.
//!
//! Keep `///` comments in this file strictly agent-facing: the `#[tool]` macro compiles
//! them into the tool schemas, so anything written here is read by the agent as
//! documentation. Implementation rationale belongs in `//` comments like this one.

use std::collections::HashSet;

// rmcp re-exports schemars, so the derives resolve without a direct dependency.
use rmcp::schemars;

use crate::hocr_parser::{HocrBlockMetrics, HocrCarea, HocrPage};
use crate::mcp::features::{self, BlockFeatures, PageStats};
use crate::mcp::pages::PageRef;
use crate::routes::projects::models::Project;

// ── PROJECTS ─────────────────────────────────────────────────────────────────

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

/// The projects available in this AetherGraph instance.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ProjectList {
    /// Every project in this AetherGraph instance, sorted by `machine_name`.
    pub projects: Vec<ProjectSummary>,
}

/// One project in detail, including the flows and layouts it declares.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct ProjectDetail {
    #[serde(flatten)]
    pub summary: ProjectSummary,
    /// Names of the text flows defined for this project. A flow is an independent
    /// reading stream through the book: body text, footnotes and callouts are separate
    /// flows, so a footnote does not interrupt the sentence it hangs off.
    pub flows: Vec<String>,
    /// Names of the layouts defined for this project.
    pub layouts: Vec<String>,
    /// Total number of pages registered for this project.
    pub page_count: usize,
    /// How many of those pages have been OCR'd and so have hOCR to inspect. Pages
    /// without hOCR can still be looked at with `get_page_image`.
    pub pages_with_hocr: usize,
}

/// Format a date the same way the `optional_date` serde helper in
/// `routes::projects::models` does, so MCP and the REST API agree.
fn format_date(date: time::Date) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        date.month() as u8,
        date.day()
    )
}

/// Treat a blank string as absent.
///
/// Clearing a field in the UI leaves `description = ""` in `project.toml` rather than
/// removing the key, and "has no description" and "has an empty description" should not
/// look different to an agent.
fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.trim().is_empty())
}

fn summarise_one(p: Project) -> ProjectSummary {
    ProjectSummary {
        machine_name: p.machine_name,
        name: p.name,
        abbrev: non_empty(p.abbrev),
        description: non_empty(p.description),
        authors: p.authors.into_iter().map(|a| a.full_name).collect(),
        published: p.published.map(format_date),
        ocr_language: non_empty(p.ocr_language),
    }
}

/// Map domain projects onto the agent-facing summaries.
///
/// Sorted by `machine_name` because `storage::read_projects` yields `fs::read_dir` order,
/// which is not stable across machines or runs; agents do better with deterministic
/// output.
pub fn summarise(projects: Vec<Project>) -> ProjectList {
    let mut projects: Vec<ProjectSummary> = projects.into_iter().map(summarise_one).collect();
    projects.sort_by(|a, b| a.machine_name.cmp(&b.machine_name));
    ProjectList { projects }
}

pub fn detail(project: Project, page_count: usize, pages_with_hocr: usize) -> ProjectDetail {
    let flows = project.flows.iter().map(|f| f.name.clone()).collect();
    let layouts = project.layouts.iter().map(|l| l.name.clone()).collect();
    ProjectDetail {
        summary: summarise_one(project),
        flows,
        layouts,
        page_count,
        pages_with_hocr,
    }
}

// ── PAGES ────────────────────────────────────────────────────────────────────

/// One page of a project.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct PageSummary {
    /// The page's identifier, taken from its scan filename, e.g. `prolog-5`. This is
    /// what every other tool means by `page`, and what appears in folio URLs.
    pub stem: String,
    /// The page number as printed in the book, e.g. `vi` or `11`. A label, not a number:
    /// front matter is commonly numbered with Roman numerals. It does NOT generally
    /// match `stem` — page `vi` may be `prolog-5`, and page `11` may be `side-012`.
    pub name: String,
    /// Scan filename on disk.
    pub scan: String,
    /// Whether this page has been OCR'd. Without hOCR there are no blocks to inspect or
    /// retype, though `get_page_image` still works.
    pub has_hocr: bool,
    /// Whether this page has edits that differ from the original OCR output.
    pub has_unsaved_edits: bool,
}

/// The pages of one project, in reading order.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct PageList {
    pub machine_name: String,
    /// Pages in book order. Use this to translate a printed page number into a `stem`.
    pub pages: Vec<PageSummary>,
}

// ── PAGE CONTENT ─────────────────────────────────────────────────────────────

/// How a block looks on the page, as measurements.
///
/// Ratios are relative to this page's own median, so `1.0` means "ordinary for this
/// page". Values above about `1.4` on several size measures at once, especially combined
/// with a high `centered` or a large `gap_above`, indicate display type such as a
/// heading.
///
/// IMPORTANT: no single size measure here is reliable. On real scans Tesseract's
/// `x_size` varies by around 15% across lines set in the same font, and the derived
/// `rel_ascent` is no better. They separate large display type confidently, and they do
/// NOT reliably separate a subsection heading from body text. When the numbers are close,
/// confirm against the scan with `get_page_image` or `get_block_image`.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct FeaturesView {
    /// Median line height of this block ÷ the page median.
    pub rel_line_height: Option<f32>,
    /// Median ascender height (baseline to top of line) ÷ the page median.
    pub rel_ascent: Option<f32>,
    /// Median Tesseract `x_size` ÷ the page median. The noisiest of the three size
    /// measures; do not rank on it alone.
    pub rel_x_size: Option<f32>,
    /// Median baseline-to-baseline distance inside this block, in scan pixels.
    /// `null` for a single-line block, which most headings are.
    pub leading: Option<f32>,
    /// `leading` ÷ the page's body leading.
    pub rel_leading: Option<f32>,
    /// How centred the block is in its column: `1.0` dead centre, `0.0` flush to an edge.
    pub centered: Option<f32>,
    /// Left offset from the column's left edge, in scan pixels. A positive value on a
    /// body paragraph usually means a first-line indent.
    pub indent: i32,
    /// Vertical gap to the previous block in the same column, in units of body leading.
    /// `null` for the first block of a column. Headings usually have more space above
    /// them than body paragraphs do.
    pub gap_above: Option<f32>,
    /// True when the text contains letters and none are lower case, e.g.
    /// `"II - NOBODY HOME"`. A common heading convention.
    pub all_caps: bool,
    /// Mean OCR confidence (0-100) over the block's words. A low value means the text is
    /// unreliable, so do not read structure into it.
    pub mean_confidence: Option<f32>,
}

/// Paragraph-continuation measurements, present only once a page has been analysed in
/// Bridge mode. Absent for most pages; do not depend on it.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct MetricsView {
    pub x_indent: i32,
    pub x_dedent: i32,
    pub y_advance: Option<i32>,
    pub y_reverse: Option<i32>,
    /// Whether the block's last word ends in a hyphen, i.e. a word broken across a
    /// boundary.
    pub has_final_hyphen: bool,
}

/// A block of text: in hOCR terms a paragraph-level element, which is also the unit that
/// carries a structural type such as `chapter` or `section`.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct BlockView {
    /// Identifier of this block, e.g. `block_1_4`. Pass this where a tool wants an id.
    pub id: String,
    /// The block's structural type: `paragraph`, `part`, `chapter`, `section`,
    /// `subsection`, `subsubsection`, `subsubsubsection`, `subsubsubsubsection`,
    /// `image`, `table` or `list`.
    pub kind: String,
    /// Bounding box `[left, top, right, bottom]` in ORIGINAL scan pixels. These
    /// coordinates are never scaled, including when an image tool returns a downscaled
    /// picture. Take geometry from here, never measured off an image.
    pub bbox: [i32; 4],
    /// The block's OCR text, words joined by single spaces.
    pub text: String,
    pub line_count: usize,
    pub word_count: usize,
    pub features: FeaturesView,
    pub metrics: Option<MetricsView>,
    /// True when this same text appears at the same height on a nearby page, which
    /// almost always means a running header or footer rather than real content.
    pub repeats_across_pages: bool,
}

/// A content area: hOCR's column-or-region grouping, one level above blocks. On a
/// multi-column page each column is normally its own carea.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct CareaView {
    /// Identifier of this carea, e.g. `block_1_1`.
    pub id: String,
    /// Which text flow this carea belongs to, if assigned. `null` means unassigned,
    /// which is normal for a page that has not been through Bridge mode yet.
    pub flow: Option<String>,
    /// Which layout this carea belongs to, if assigned.
    pub layout: Option<String>,
    /// Bounding box `[left, top, right, bottom]` in original scan pixels.
    pub bbox: [i32; 4],
    pub blocks: Vec<BlockView>,
}

/// One page's structure, with visual measurements for every block.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct PageView {
    pub machine_name: String,
    /// The page identifier used by the tools, from the scan filename.
    pub stem: String,
    /// The page number as printed in the book. A label, not a number.
    pub name: String,
    pub scan: String,
    pub has_unsaved_edits: bool,
    /// The page's own bounding box `[left, top, right, bottom]` in scan pixels.
    pub page_bbox: [i32; 4],
    /// Median baseline-to-baseline distance of this page's body text, in scan pixels.
    /// The reference that `rel_leading` and `gap_above` are measured in.
    pub body_leading: Option<f32>,
    /// Median line height across the page, in scan pixels.
    pub median_line_height: Option<f32>,
    /// Content areas in document order. On a multi-column page, reading order runs down
    /// each carea in turn, NOT across the page — so do not order blocks by their `top`
    /// coordinate across different careas.
    pub careas: Vec<CareaView>,
}

fn metrics_view(m: &HocrBlockMetrics) -> MetricsView {
    MetricsView {
        x_indent: m.x_indent,
        x_dedent: m.x_dedent,
        y_advance: m.y_advance,
        y_reverse: m.y_reverse,
        has_final_hyphen: m.has_final_hyphen,
    }
}

fn features_view(f: BlockFeatures) -> FeaturesView {
    FeaturesView {
        rel_line_height: f.rel_line_height,
        rel_ascent: f.rel_ascent,
        rel_x_size: f.rel_x_size,
        leading: f.leading,
        rel_leading: f.rel_leading,
        centered: f.centered,
        indent: f.indent,
        gap_above: f.gap_above,
        all_caps: f.all_caps,
        mean_confidence: f.mean_confidence,
    }
}

fn carea_view(carea: &HocrCarea, stats: &PageStats, repeats: &HashSet<String>) -> CareaView {
    let blocks = carea
        .blocks
        .iter()
        .enumerate()
        .map(|(i, block)| BlockView {
            id: block.id.clone(),
            kind: block.kind.json_name().to_string(),
            bbox: block.bbox.0,
            text: features::block_text(block),
            line_count: block.lines.len(),
            word_count: block.lines.iter().map(|l| l.words.len()).sum(),
            features: features_view(
                features::block_features(carea, i, stats).unwrap_or_default(),
            ),
            metrics: block.metrics.as_ref().map(metrics_view),
            repeats_across_pages: repeats.contains(&block.id),
        })
        .collect();

    CareaView {
        id: carea.id.clone(),
        flow: carea.flow.clone(),
        layout: carea.layout.clone(),
        bbox: carea.bbox.0,
        blocks,
    }
}

/// Build the agent-facing view of a parsed page.
pub fn page_view(
    machine_name: &str,
    page_ref: &PageRef,
    has_unsaved_edits: bool,
    page: &HocrPage,
    neighbours: &[HocrPage],
) -> PageView {
    let stats = features::page_stats(page);
    let repeats = features::repeating_block_ids(page, neighbours);

    PageView {
        machine_name: machine_name.to_string(),
        stem: page_ref.stem.clone(),
        name: page_ref.name.clone(),
        scan: page_ref.scan.clone(),
        has_unsaved_edits,
        page_bbox: page.bbox.0,
        body_leading: stats.body_leading,
        median_line_height: stats.median_line_height,
        careas: page
            .careas
            .iter()
            .map(|c| carea_view(c, &stats, &repeats))
            .collect(),
    }
}

// ── FOCUS ────────────────────────────────────────────────────────────────────

/// Confirmation of where the user interface has been pointed.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct FocusView {
    pub machine_name: String,
    /// The page the UI was moved to, by stem.
    pub stem: String,
    /// The page number as printed in the book, for talking to the user about it.
    pub name: String,
    pub mode: String,
    /// The URL now showing in the user's browser.
    pub url: String,
}
