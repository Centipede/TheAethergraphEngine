//! Keeps the structure database in step with the hOCR on disk.
//!
//! `Documentation/Planning/AetherGraph.md` requires that "all operations that
//! can cause changes to potentially involved (page, block-id) for these
//! sections must load the structure database and update both sections and
//! headlines". This module is that update.
//!
//! # Reconcile, don't diff
//!
//! Rather than asking each handler to describe what it changed, we recompute a
//! page's headline rows from the page's current block set. Block removal
//! cascades invisibly -- [`crate::hocr_parser::HocrPage::cleanup_line`] ->
//! `cleanup_block` -> `cleanup_carea` -- so deleting a block's last word
//! deletes the block, which makes `word_remove` a block-removal operation.
//! Merges discard every id but the first, splits hard-code the new half to
//! `Paragraph`, and rescans renumber whole subtrees. A state-based reconcile
//! covers all of these, and any future operation, without enumerating them.
//!
//! It is also idempotent, which is what lets a section un-orphan itself when a
//! block is turned back into a heading.
//!
//! # What this module owns
//!
//! Exactly three things: membership of [`StructureDb::headlines`],
//! [`Headline::is_linked`], and [`Section::is_orphaned`]. Section titles,
//! path ids, kinds, nesting and `is_suggested` belong to Codex. A section whose
//! headline disappears is **orphaned, never removed** -- deleting it is a
//! decision for the user.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::hocr_parser::{HocrPage, ParserConfig};
use crate::routes::projects::models::{Headline, PageDb, Section, StructureDb};
use crate::routes::projects::storage;

// ── PURE CORE ────────────────────────────────────────────────────────

/// The headline-kind blocks of one page, in document order.
#[derive(Debug, Clone, PartialEq)]
pub struct PageHeadlines {
    pub page_name: String,
    pub block_ids: Vec<String>,
}

/// Collect the headline-kind blocks of an already-parsed page.
pub fn page_headlines(page: &HocrPage, page_name: &str) -> PageHeadlines {
    PageHeadlines {
        page_name: page_name.to_string(),
        block_ids: page
            .careas
            .iter()
            .flat_map(|carea| carea.blocks.iter())
            .filter(|block| block.kind.is_headline())
            .map(|block| block.id.clone())
            .collect(),
    }
}

/// Reconcile `db` against the live state of the given pages.
///
/// Each entry must list *all* headline blocks of that page, since anything
/// absent from it is taken to be gone. Pages with an empty name are skipped:
/// the structure database is keyed by page name, so an unnamed page cannot be
/// represented. Returns whether `db` was modified.
pub fn reconcile_pages(db: &mut StructureDb, pages: &[PageHeadlines]) -> bool {
    let mut changed = false;
    for page in pages {
        if page.page_name.is_empty() {
            continue;
        }
        changed |= reconcile_one_page(db, page);
    }
    changed
}

fn reconcile_one_page(db: &mut StructureDb, page: &PageHeadlines) -> bool {
    let live: HashSet<&str> = page.block_ids.iter().map(String::as_str).collect();
    let mut changed = false;

    // 1. Rebuild this page's rows from the live block list, rather than
    //    deleting and appending. That keeps them in document order even when a
    //    heading is removed and later restored, and drops anything whose block
    //    is gone or is no longer a headline kind.
    //
    //    Rows are spliced back where the page's first row was, so pages stay
    //    grouped in the file instead of drifting into edit order.
    let at = db
        .headlines
        .iter()
        .position(|row| row.page == page.page_name)
        .unwrap_or(db.headlines.len());
    let old_rows: Vec<Headline> = {
        let mut old_rows = Vec::new();
        db.headlines.retain(|row| {
            if row.page == page.page_name {
                old_rows.push(row.clone());
                false
            } else {
                true
            }
        });
        old_rows
    };
    let was_linked: HashMap<&str, bool> = old_rows
        .iter()
        .map(|row| (row.block_id.as_str(), row.is_linked))
        .collect();
    let mut seen = HashSet::new();
    let new_rows: Vec<Headline> = page
        .block_ids
        .iter()
        .filter(|id| seen.insert(id.as_str()))
        .map(|block_id| Headline {
            page: page.page_name.clone(),
            block_id: block_id.clone(),
            is_linked: was_linked
                .get(block_id.as_str())
                .copied()
                .unwrap_or(false),
        })
        .collect();
    changed |= new_rows != old_rows;
    db.headlines.splice(at..at, new_rows);

    // 2. Orphan sections whose headline has gone -- and un-orphan those whose
    //    headline is back. One assignment does both, which is why the whole
    //    reconcile has to be a pure function of live page state.
    changed |= set_orphaned_for_page(&mut db.sections, &page.page_name, &live);

    // 3. Recompute is_linked now that step 2 has settled. Must come last.
    let linked = section_headline_keys(&db.sections);
    for row in db.headlines.iter_mut().filter(|r| r.page == page.page_name) {
        let is_linked = linked.contains(&(row.page.as_str(), row.block_id.as_str()));
        if row.is_linked != is_linked {
            row.is_linked = is_linked;
            changed = true;
        }
    }

    changed
}

fn set_orphaned_for_page(
    sections: &mut [Section],
    page_name: &str,
    live: &HashSet<&str>,
) -> bool {
    let mut changed = false;
    for section in sections.iter_mut() {
        if let Some(headline) = &section.headline {
            if headline.page == page_name {
                let orphaned = !live.contains(headline.block_id.as_str());
                if section.is_orphaned != orphaned {
                    section.is_orphaned = orphaned;
                    changed = true;
                }
            }
        }
        changed |= set_orphaned_for_page(&mut section.subsections, page_name, live);
    }
    changed
}

/// Every `(page, block_id)` a section currently points at.
///
/// `Headline` has no back-pointer to its section -- the planning doc specifies
/// a `linked_section` path, but the model carries only a bool -- so resolving
/// the link means walking the tree.
fn section_headline_keys(sections: &[Section]) -> HashSet<(&str, &str)> {
    let mut out = HashSet::new();
    collect_section_headline_keys(sections, &mut out);
    out
}

fn collect_section_headline_keys<'a>(
    sections: &'a [Section],
    out: &mut HashSet<(&'a str, &'a str)>,
) {
    for section in sections {
        if let Some(headline) = &section.headline {
            out.insert((headline.page.as_str(), headline.block_id.as_str()));
        }
        collect_section_headline_keys(&section.subsections, out);
    }
}

/// Apply page renames and drop rows for pages that no longer exist.
///
/// `renames` maps old page name to new. It is applied as a **single
/// simultaneous pass**, not a chain of substitutions: renumbering a book is a
/// permutation, so `p1 -> p2` can coincide with another page becoming `p1`, and
/// rewriting one row at a time would cascade them into each other.
///
/// `live_page_names` is the set of page names that still exist afterwards.
/// Returns whether `db` was modified.
pub fn remap_and_prune_pages(
    db: &mut StructureDb,
    renames: &HashMap<String, String>,
    live_page_names: &HashSet<String>,
) -> bool {
    let mut changed = false;

    for row in db.headlines.iter_mut() {
        if let Some(new_name) = renames.get(&row.page) {
            if &row.page != new_name {
                row.page = new_name.clone();
                changed = true;
            }
        }
    }
    changed |= remap_sections(&mut db.sections, renames);

    let before = db.headlines.len();
    db.headlines
        .retain(|row| live_page_names.contains(&row.page));
    changed |= db.headlines.len() != before;

    // A section whose page is gone is orphaned, not deleted.
    changed |= orphan_sections_off_page(&mut db.sections, live_page_names);

    changed
}

fn remap_sections(sections: &mut [Section], renames: &HashMap<String, String>) -> bool {
    let mut changed = false;
    for section in sections.iter_mut() {
        if let Some(headline) = &mut section.headline {
            if let Some(new_name) = renames.get(&headline.page) {
                if &headline.page != new_name {
                    headline.page = new_name.clone();
                    changed = true;
                }
            }
        }
        changed |= remap_sections(&mut section.subsections, renames);
    }
    changed
}

fn orphan_sections_off_page(
    sections: &mut [Section],
    live_page_names: &HashSet<String>,
) -> bool {
    let mut changed = false;
    for section in sections.iter_mut() {
        if let Some(headline) = &section.headline {
            if !live_page_names.contains(&headline.page) && !section.is_orphaned {
                section.is_orphaned = true;
                changed = true;
            }
        }
        changed |= orphan_sections_off_page(&mut section.subsections, live_page_names);
    }
    changed
}

/// Build the old-name -> new-name map from two page-database snapshots.
///
/// Keyed on `Page.scan`, which never changes ("No scanned file is ever changed
/// after upload"), so it survives arbitrary renames and reorderings. Old names
/// that were shared by more than one page are skipped: their structure rows
/// cannot be attributed to one page, and guessing would corrupt them.
pub fn page_renames(old: &PageDb, new: &PageDb) -> HashMap<String, String> {
    let new_by_scan: HashMap<&str, &str> = new
        .pages
        .iter()
        .map(|page| (stem_of(&page.scan), page.name.as_str()))
        .collect();

    let mut occurrences: HashMap<&str, usize> = HashMap::new();
    for page in &old.pages {
        *occurrences.entry(page.name.as_str()).or_insert(0) += 1;
    }

    let mut renames = HashMap::new();
    for page in &old.pages {
        if page.name.is_empty() || occurrences.get(page.name.as_str()) != Some(&1) {
            continue;
        }
        let Some(new_name) = new_by_scan.get(stem_of(&page.scan)) else {
            continue; // page was removed; pruning deals with it
        };
        if new_name.is_empty() || *new_name == page.name.as_str() {
            continue;
        }
        renames.insert(page.name.clone(), new_name.to_string());
    }
    renames
}

/// Page names that can key structure rows: non-empty and unambiguous.
fn usable_page_names(page_db: &PageDb) -> HashSet<String> {
    let mut occurrences: HashMap<&str, usize> = HashMap::new();
    for page in &page_db.pages {
        *occurrences.entry(page.name.as_str()).or_insert(0) += 1;
    }
    page_db
        .pages
        .iter()
        .filter(|page| !page.name.is_empty() && occurrences.get(page.name.as_str()) == Some(&1))
        .map(|page| page.name.clone())
        .collect()
}

fn stem_of(scan: &str) -> &str {
    Path::new(scan)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(scan)
}

/// Resolve a scan stem to the page name the structure database is keyed by.
///
/// `None` when the page is unknown, unnamed, or shares its name with another
/// page -- in all three cases we leave the structure database alone rather than
/// write rows we could not reliably find again.
fn page_name_for_stem(page_db: &PageDb, stem: &str) -> Option<String> {
    let stem = stem_of(stem);
    let page = page_db
        .pages
        .iter()
        .find(|page| stem_of(&page.scan) == stem)?;
    if page.name.is_empty() {
        return None;
    }
    let shared = page_db
        .pages
        .iter()
        .filter(|other| other.name == page.name)
        .count();
    if shared > 1 {
        return None;
    }
    Some(page.name.clone())
}

// ── I/O WRAPPER ──────────────────────────────────────────────────────

/// Load, mutate and save the structure database, writing only on a real change.
///
/// Best-effort by design: the hOCR write that triggered this has already
/// succeeded, so a structure-sync failure must not turn a successful edit into
/// a 500. Failures are logged. A structure database that will not parse aborts
/// the write entirely, so a transient read problem cannot flatten the user's
/// section tree.
fn apply<F>(projects_dir: &PathBuf, machine_name: &str, mutate: F)
where
    F: FnOnce(&mut StructureDb) -> bool,
{
    let path = storage::structuredb_path(projects_dir, machine_name);
    let mut db = match storage::try_load_structure_db(&path) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("structure sync: not writing {}: {e}", path.display());
            return;
        }
    };
    if !mutate(&mut db) {
        return;
    }
    if let Err(e) = storage::save_structure_db_atomic(&path, &db) {
        eprintln!("structure sync: failed to save {}: {e}", path.display());
    }
}

fn load_page_db(projects_dir: &PathBuf, machine_name: &str) -> PageDb {
    storage::load_page_db(&storage::pagedb_path(projects_dir, machine_name))
}

/// Reconcile one page from a page already parsed in memory.
///
/// This is the hot path: it runs after every hOCR edit, so it deliberately
/// avoids re-reading and re-parsing the file the caller just wrote.
pub fn sync_page(projects_dir: &PathBuf, machine_name: &str, stem: &str, page: &HocrPage) {
    let page_db = load_page_db(projects_dir, machine_name);
    let Some(page_name) = page_name_for_stem(&page_db, stem) else {
        return;
    };
    let headlines = page_headlines(page, &page_name);
    apply(projects_dir, machine_name, |db| {
        reconcile_pages(db, std::slice::from_ref(&headlines))
    });
}

/// Reconcile several pages, re-reading each from disk.
///
/// For callers that rewrote hOCR without keeping the parsed page, or that
/// touched many pages at once. The structure database is loaded and saved once
/// for the whole batch.
pub fn sync_stems(projects_dir: &PathBuf, machine_name: &str, stems: &[String]) {
    let page_db = load_page_db(projects_dir, machine_name);
    let collected = collect_from_disk(projects_dir, machine_name, &page_db, stems);
    if collected.is_empty() {
        return;
    }
    apply(projects_dir, machine_name, |db| {
        reconcile_pages(db, &collected)
    });
}

fn collect_from_disk(
    projects_dir: &PathBuf,
    machine_name: &str,
    page_db: &PageDb,
    stems: &[String],
) -> Vec<PageHeadlines> {
    let mut collected = Vec::new();
    for stem in stems {
        let Some(page_name) = page_name_for_stem(page_db, stem) else {
            continue;
        };
        let Some(path) = storage::hocr_active_path(projects_dir, machine_name, stem) else {
            continue;
        };
        let Ok(html) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some(parsed) = crate::hocr_parser::parse(&html, ParserConfig::default()) else {
            continue;
        };
        collected.push(page_headlines(&parsed, &page_name));
    }
    collected
}

/// Handle a whole-`PageDb` replacement: renames, removals and newly-named pages.
///
/// `old` must be the database as it was before the write. Pages whose name
/// changed are re-read from hOCR, which is also how headings marked on a page
/// before it was named get picked up once a name exists.
pub fn sync_page_db_change(
    projects_dir: &PathBuf,
    machine_name: &str,
    old: &PageDb,
    new: &PageDb,
) {
    let renames = page_renames(old, new);
    let live = usable_page_names(new);

    let old_names: HashMap<&str, &str> = old
        .pages
        .iter()
        .map(|page| (stem_of(&page.scan), page.name.as_str()))
        .collect();
    let renamed_stems: Vec<String> = new
        .pages
        .iter()
        .filter(|page| {
            old_names
                .get(stem_of(&page.scan))
                .is_none_or(|old_name| *old_name != page.name.as_str())
        })
        .map(|page| stem_of(&page.scan).to_string())
        .collect();

    let collected = collect_from_disk(projects_dir, machine_name, new, &renamed_stems);

    if renames.is_empty() && collected.is_empty() {
        // Still worth a pass: pages may have been removed outright.
        apply(projects_dir, machine_name, |db| {
            remap_and_prune_pages(db, &renames, &live)
        });
        return;
    }

    apply(projects_dir, machine_name, |db| {
        let mut changed = remap_and_prune_pages(db, &renames, &live);
        changed |= reconcile_pages(db, &collected);
        changed
    });
}

#[cfg(test)]
mod tests;
