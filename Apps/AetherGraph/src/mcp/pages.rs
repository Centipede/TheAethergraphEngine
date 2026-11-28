//! Page identification and image preparation for the MCP tools.
//!
//! The whole reason this module exists: a page has two names, and confusing them
//! silently works on the wrong page. `Page.scan` gives the *stem* (`prolog-5`), which is
//! what every hOCR path and API route uses. `Page.name` is the *printed* label the reader
//! sees, which is a string, not a number — the front matter of `other-project` is numbered
//! `ii`, `iii`, … `ix`. Humans say "the TOC is on page vi"; the filesystem says
//! `prolog-5`. Tools accept either and echo back both.

use std::path::Path;

use crate::routes::projects::models::Page;

/// One page, identified both ways.
#[derive(Debug, Clone, PartialEq)]
pub struct PageRef {
    pub stem: String,
    pub name: String,
    pub scan: String,
}

/// Why a page query could not be turned into exactly one page.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolveError {
    /// Nothing matched, with a sample of what does exist.
    NotFound { query: String, known: Vec<String> },
    /// Several pages carry the same printed name; the caller must use a stem.
    Ambiguous { query: String, stems: Vec<String> },
}

impl ResolveError {
    /// Message written for the agent, so it can correct itself without a round trip.
    pub fn message(&self) -> String {
        match self {
            ResolveError::NotFound { query, known } => format!(
                "No page matches '{query}'. Pages are addressed by scan stem (e.g. \
                 prolog-5) or by printed page name (e.g. vi). Known pages include: {}. \
                 Call list_pages for the full mapping.",
                known.join(", ")
            ),
            ResolveError::Ambiguous { query, stems } => format!(
                "'{query}' is the printed name of more than one page ({}). Use one of \
                 those scan stems instead.",
                stems.join(", ")
            ),
        }
    }
}

/// The scan stem: `prolog-5.jpg` -> `prolog-5`.
pub fn stem_of(scan: &str) -> String {
    Path::new(scan)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(scan)
        .to_string()
}

/// Resolve a stem or a printed page name to exactly one page.
///
/// Stems are tried first because they are the canonical identifier; a printed name that
/// happens to look like a stem therefore cannot shadow a real one.
pub fn resolve_page(pages: &[Page], query: &str) -> Result<PageRef, ResolveError> {
    let q = query.trim();

    let to_ref = |p: &Page| PageRef {
        stem: stem_of(&p.scan),
        name: p.name.clone(),
        scan: p.scan.clone(),
    };

    if let Some(p) = pages.iter().find(|p| stem_of(&p.scan) == q) {
        return Ok(to_ref(p));
    }

    let by_name: Vec<&Page> = pages
        .iter()
        .filter(|p| p.name.trim().eq_ignore_ascii_case(q))
        .collect();

    match by_name.as_slice() {
        [p] => Ok(to_ref(p)),
        [] => Err(ResolveError::NotFound {
            query: q.to_string(),
            known: pages
                .iter()
                .take(8)
                .map(|p| format!("{} (page {})", stem_of(&p.scan), p.name))
                .collect(),
        }),
        many => Err(ResolveError::Ambiguous {
            query: q.to_string(),
            stems: many.iter().map(|p| stem_of(&p.scan)).collect(),
        }),
    }
}

// ── IMAGE PREPARATION ────────────────────────────────────────────────────────

/// Longest edge, in pixels, of an image handed to an agent when none is requested.
pub const DEFAULT_SCALE: u32 = 1400;
/// Upper bound on the requested longest edge. A three-column TOC is the case that wants
/// the top of this range; beyond it the token cost stops buying legibility.
pub const MAX_SCALE: u32 = 2000;

/// Target pixel size for an image whose longest edge should become `max_edge`.
///
/// Never upscales: asking for 2000 on a 900px scan returns the 900px original rather than
/// an interpolated fake. Returns `None` when no resizing is needed.
pub fn fit_to_long_edge(width: u32, height: u32, max_edge: u32) -> Option<(u32, u32)> {
    let max_edge = max_edge.clamp(1, MAX_SCALE);
    let longest = width.max(height);
    if longest <= max_edge || longest == 0 {
        return None;
    }
    let factor = max_edge as f64 / longest as f64;
    Some((
        ((width as f64 * factor).round() as u32).max(1),
        ((height as f64 * factor).round() as u32).max(1),
    ))
}

/// Downscale if needed and encode as JPEG, returning the bytes and the final size.
///
/// JPEG rather than PNG on purpose: these are photographs of paper, where PNG is several
/// times larger for no readability gain.
pub fn to_jpeg(bytes: &[u8], max_edge: u32) -> Result<(Vec<u8>, u32, u32, u32, u32), String> {
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let (ow, oh) = (img.width(), img.height());

    let img = match fit_to_long_edge(ow, oh, max_edge) {
        Some((w, h)) => img.resize(w, h, image::imageops::FilterType::Lanczos3),
        None => img,
    };
    let (w, h) = (img.width(), img.height());

    let mut out = std::io::Cursor::new(Vec::new());
    img.into_rgb8()
        .write_to(&mut out, image::ImageFormat::Jpeg)
        .map_err(|e| e.to_string())?;

    Ok((out.into_inner(), ow, oh, w, h))
}

/// The folio modes the UI can be driven to, i.e. the `folios-*` route names in
/// `AetherGraph-UI/src/router/router.ts`.
pub const FOLIO_MODES: [&str; 6] = ["assist", "crop", "hint", "recognise", "edit", "bridge"];

/// Validate a mode name against the routes that actually exist, so `set_focus` cannot
/// park the UI on a route that 404s.
pub fn validate_mode(mode: &str) -> Result<String, String> {
    let m = mode.trim().to_ascii_lowercase();
    if FOLIO_MODES.contains(&m.as_str()) {
        Ok(m)
    } else {
        Err(format!(
            "Unknown mode '{mode}'. Valid folio modes are: {}.",
            FOLIO_MODES.join(", ")
        ))
    }
}
