//! Visual features derived from hOCR geometry, so an agent can reason about what a block
//! *looks* like without being handed a picture of it.
//!
//! Three things shaped this module, all learned from real scans:
//!
//! 1. **No single size signal is trustworthy.** Across the 88 lines of
//!    `other-project/prolog-5`, which are nearly all one body font, Tesseract's `x_size`
//!    ranges 39.4–53.0 around a median of 48 — ±15% noise on identical type. A derived
//!    ascent is no better (median 38, stdev 5.9). Both separate *large display type*
//!    cleanly and neither separates a subsection heading from body text. So we report
//!    several ratios and let the caller weigh them against the scan, rather than
//!    pretending one number decides.
//! 2. **Line order across blocks is meaningless on a multi-column page.** In that same
//!    TOC the three columns interleave, so consecutive lines in document order sit in
//!    different columns and naive gaps come out negative. Leading is therefore measured
//!    only *within a block*, and `gap_above` only *within a carea*.
//! 3. **Most headings are a single line**, which is exactly when leading is unavailable.
//!    That limitation is real and is stated in the tool description rather than papered
//!    over.

use std::collections::HashSet;

use crate::hocr_parser::{HocrBlock, HocrCarea, HocrLine, HocrPage};

/// Page-level reference values that per-block ratios are measured against.
#[derive(Debug, Clone, PartialEq)]
pub struct PageStats {
    pub median_line_height: Option<f32>,
    pub median_ascent: Option<f32>,
    pub median_x_size: Option<f32>,
    /// Median baseline-to-baseline distance of body text. See [`body_leading`].
    pub body_leading: Option<f32>,
}

/// What a block looks like, as numbers.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BlockFeatures {
    pub rel_line_height: Option<f32>,
    pub rel_ascent: Option<f32>,
    pub rel_x_size: Option<f32>,
    pub leading: Option<f32>,
    pub rel_leading: Option<f32>,
    pub centered: Option<f32>,
    pub indent: i32,
    pub gap_above: Option<f32>,
    pub all_caps: bool,
    pub mean_confidence: Option<f32>,
}

// ── PRIMITIVES ───────────────────────────────────────────────────────────────

fn median(values: &mut Vec<f32>) -> Option<f32> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = values.len() / 2;
    Some(if values.len() % 2 == 0 {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    })
}

/// Absolute y of a line's baseline.
///
/// hOCR writes `baseline <slope> <offset>` where the offset is relative to the bottom of
/// the line's bbox (see `HocrLine::baseline`), so this is bbox bottom plus that offset.
/// The slope is ignored: it describes page skew, not vertical position.
pub fn baseline_y(line: &HocrLine) -> Option<f32> {
    line.baseline
        .map(|(_slope, offset)| line.bbox.bottom() as f32 + offset)
}

fn line_height(line: &HocrLine) -> f32 {
    (line.bbox.bottom() - line.bbox.top()) as f32
}

/// Height from baseline up to the top of the line box, i.e. ascender height. A size proxy
/// that does not depend on Tesseract's `x_size` estimate.
fn ascent(line: &HocrLine) -> Option<f32> {
    baseline_y(line).map(|b| b - line.bbox.top() as f32)
}

/// Baseline-to-baseline distances between consecutive lines of one block.
///
/// Only positive gaps are kept. A non-positive gap means the lines are not in reading
/// order relative to each other, which happens when OCR groups across a column break;
/// such a pair carries no leading information and is dropped rather than averaged in.
fn intra_block_gaps(block: &HocrBlock) -> Vec<f32> {
    block
        .lines
        .windows(2)
        .filter_map(|w| match (baseline_y(&w[0]), baseline_y(&w[1])) {
            (Some(a), Some(b)) if b - a > 0.0 => Some(b - a),
            _ => None,
        })
        .collect()
}

/// Median leading inside a block, or `None` for a single-line block.
pub fn block_leading(block: &HocrBlock) -> Option<f32> {
    let mut gaps = intra_block_gaps(block);
    median(&mut gaps)
}

/// The page's body leading.
///
/// Every intra-block line gap on the page is pooled into one sample and the median taken.
/// Pooling the raw gaps — rather than taking a median of per-block medians — weights the
/// result by line count for free, so multi-line body paragraphs dominate and a stray
/// two-line heading cannot drag the reference value around.
pub fn body_leading(page: &HocrPage) -> Option<f32> {
    let mut gaps: Vec<f32> = page
        .careas
        .iter()
        .flat_map(|c| c.blocks.iter())
        .flat_map(intra_block_gaps)
        .collect();
    median(&mut gaps)
}

/// Collect the page-level reference values.
pub fn page_stats(page: &HocrPage) -> PageStats {
    let lines: Vec<&HocrLine> = page
        .careas
        .iter()
        .flat_map(|c| c.blocks.iter())
        .flat_map(|b| b.lines.iter())
        .collect();

    let mut heights: Vec<f32> = lines.iter().map(|l| line_height(l)).collect();
    let mut ascents: Vec<f32> = lines.iter().filter_map(|l| ascent(l)).collect();
    let mut x_sizes: Vec<f32> = lines.iter().filter_map(|l| l.x_size).collect();

    PageStats {
        median_line_height: median(&mut heights),
        median_ascent: median(&mut ascents),
        median_x_size: median(&mut x_sizes),
        body_leading: body_leading(page),
    }
}

// ── TEXT ─────────────────────────────────────────────────────────────────────

/// A block's text, words joined by spaces across all its lines.
pub fn block_text(block: &HocrBlock) -> String {
    let words: Vec<&str> = block
        .lines
        .iter()
        .flat_map(|l| l.words.iter())
        .map(|w| w.text.trim())
        .filter(|t| !t.is_empty())
        .collect();
    words.join(" ")
}

/// True when the text has letters and none of them are lower case.
///
/// Catches the all-caps heading convention, e.g. `"II - NOBODY HOME"`. Digits, Roman
/// numerals and punctuation do not count either way, so `"1533"` is not "all caps".
pub fn all_caps(text: &str) -> bool {
    let mut has_alpha = false;
    for c in text.chars() {
        if c.is_alphabetic() {
            has_alpha = true;
            if c.is_lowercase() {
                return false;
            }
        }
    }
    has_alpha
}

/// Mean OCR confidence over a block's words, or `None` when it has none.
///
/// Low confidence is how OCR junk announces itself; without this a garbled artefact can
/// look like a heading on every geometric measure.
pub fn mean_confidence(block: &HocrBlock) -> Option<f32> {
    let confs: Vec<f32> = block
        .lines
        .iter()
        .flat_map(|l| l.words.iter())
        .map(|w| w.wconf as f32)
        .collect();
    if confs.is_empty() {
        None
    } else {
        Some(confs.iter().sum::<f32>() / confs.len() as f32)
    }
}

// ── GEOMETRY ─────────────────────────────────────────────────────────────────

/// How centred a block is within its carea: 1.0 dead centre, 0.0 flush to an edge.
pub fn centered(block: &HocrBlock, carea: &HocrCarea) -> Option<f32> {
    let half = (carea.bbox.right() - carea.bbox.left()) as f32 / 2.0;
    if half <= 0.0 {
        return None;
    }
    let block_mid = (block.bbox.left() + block.bbox.right()) as f32 / 2.0;
    let carea_mid = (carea.bbox.left() + carea.bbox.right()) as f32 / 2.0;
    Some((1.0 - (block_mid - carea_mid).abs() / half).clamp(0.0, 1.0))
}

/// Vertical gap to the previous block *in the same carea*, in units of body leading.
///
/// `None` for the first block in a carea. Deliberately never looks at the previous carea:
/// on a multi-column page that neighbour is in another column and the "gap" is noise.
pub fn gap_above(carea: &HocrCarea, block_index: usize, body_leading: Option<f32>) -> Option<f32> {
    if block_index == 0 {
        return None;
    }
    let leading = body_leading.filter(|l| *l > 0.0)?;
    let prev = carea.blocks.get(block_index - 1)?;
    let this = carea.blocks.get(block_index)?;
    Some((this.bbox.top() - prev.bbox.bottom()) as f32 / leading)
}

/// Every feature for one block.
pub fn block_features(
    carea: &HocrCarea,
    block_index: usize,
    stats: &PageStats,
) -> Option<BlockFeatures> {
    let block = carea.blocks.get(block_index)?;

    let ratio = |value: Option<f32>, reference: Option<f32>| -> Option<f32> {
        match (value, reference) {
            (Some(v), Some(r)) if r > 0.0 => Some(v / r),
            _ => None,
        }
    };

    let mut heights: Vec<f32> = block.lines.iter().map(line_height).collect();
    let mut ascents: Vec<f32> = block.lines.iter().filter_map(ascent).collect();
    let mut x_sizes: Vec<f32> = block.lines.iter().filter_map(|l| l.x_size).collect();

    let leading = block_leading(block);

    Some(BlockFeatures {
        rel_line_height: ratio(median(&mut heights), stats.median_line_height),
        rel_ascent: ratio(median(&mut ascents), stats.median_ascent),
        rel_x_size: ratio(median(&mut x_sizes), stats.median_x_size),
        leading,
        rel_leading: ratio(leading, stats.body_leading),
        centered: centered(block, carea),
        indent: block.bbox.left() - carea.bbox.left(),
        gap_above: gap_above(carea, block_index, stats.body_leading),
        all_caps: all_caps(&block_text(block)),
        mean_confidence: mean_confidence(block),
    })
}

// ── RUNNING HEADERS AND FOOTERS ──────────────────────────────────────────────

/// Fraction of page height within which two blocks count as being "in the same place".
const REPEAT_Y_TOLERANCE: f32 = 0.04;
/// Shortest comparable text worth testing, after normalisation.
const REPEAT_MIN_CHARS: usize = 3;

/// Normalise text for cross-page comparison.
///
/// Digits are stripped because a running header usually carries the page number, which is
/// the one part guaranteed to differ. What remains is the repeating part — the book or
/// chapter title. Text that is *only* digits normalises to nothing and is skipped, which
/// is what we want: a bare folio number is not a running header worth flagging.
fn normalise_for_repeat(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphabetic() || c.is_whitespace())
        .flat_map(|c| c.to_lowercase())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn relative_y(block: &HocrBlock, page_height: i32) -> Option<f32> {
    if page_height <= 0 {
        return None;
    }
    let mid = (block.bbox.top() + block.bbox.bottom()) as f32 / 2.0;
    Some(mid / page_height as f32)
}

/// Ids of blocks on `page` whose text also appears at the same relative height on at
/// least one neighbouring page — almost always a running header or footer.
///
/// Neighbours are supplied by the caller, bounded, so this cannot walk a whole book.
pub fn repeating_block_ids(
    page: &HocrPage,
    neighbours: &[HocrPage],
) -> HashSet<String> {
    let page_height = page.bbox.bottom() - page.bbox.top();

    let neighbour_entries: Vec<(String, f32)> = neighbours
        .iter()
        .flat_map(|n| {
            let h = n.bbox.bottom() - n.bbox.top();
            n.careas
                .iter()
                .flat_map(|c| c.blocks.iter())
                .filter_map(move |b| {
                    let text = normalise_for_repeat(&block_text(b));
                    if text.len() < REPEAT_MIN_CHARS {
                        return None;
                    }
                    relative_y(b, h).map(|y| (text, y))
                })
                .collect::<Vec<_>>()
        })
        .collect();

    page.careas
        .iter()
        .flat_map(|c| c.blocks.iter())
        .filter_map(|b| {
            let text = normalise_for_repeat(&block_text(b));
            if text.len() < REPEAT_MIN_CHARS {
                return None;
            }
            let y = relative_y(b, page_height)?;
            let hit = neighbour_entries
                .iter()
                .any(|(t, ny)| *t == text && (ny - y).abs() <= REPEAT_Y_TOLERANCE);
            hit.then(|| b.id.clone())
        })
        .collect()
}
