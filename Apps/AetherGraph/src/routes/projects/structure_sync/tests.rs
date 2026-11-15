use super::*;
use crate::routes::projects::models::{Page, SectionHeadline, SectionKind, SubsectionType};

// ── BUILDERS ─────────────────────────────────────────────────────────

fn live(page: &str, block_ids: &[&str]) -> PageHeadlines {
    PageHeadlines {
        page_name: page.to_string(),
        block_ids: block_ids.iter().map(|id| id.to_string()).collect(),
    }
}

fn row(page: &str, block_id: &str, is_linked: bool) -> Headline {
    Headline {
        page: page.to_string(),
        block_id: block_id.to_string(),
        is_linked,
    }
}

fn section(path_id: &str, headline: Option<(&str, &str)>) -> Section {
    Section {
        path_id: path_id.to_string(),
        kind: SectionKind::Section,
        title: String::new(),
        is_linked: headline.is_some(),
        is_orphaned: false,
        is_suggested: false,
        headline: headline.map(|(page, block_id)| SectionHeadline {
            page: page.to_string(),
            block_id: block_id.to_string(),
        }),
        subsection_type: SubsectionType::Sections,
        subsections: vec![],
    }
}

fn page_db(pages: &[(&str, &str)]) -> PageDb {
    PageDb {
        pages: pages
            .iter()
            .map(|(scan, name)| Page {
                scan: scan.to_string(),
                name: name.to_string(),
                ..Page::default()
            })
            .collect(),
        next_batch: 0,
    }
}

/// Find a section by path_id anywhere in the tree.
fn find<'a>(sections: &'a [Section], path_id: &str) -> Option<&'a Section> {
    for section in sections {
        if section.path_id == path_id {
            return Some(section);
        }
        if let Some(found) = find(&section.subsections, path_id) {
            return Some(found);
        }
    }
    None
}

fn ids(db: &StructureDb, page_name: &str) -> Vec<String> {
    db.headlines
        .iter()
        .filter(|row| row.page == page_name)
        .map(|row| row.block_id.clone())
        .collect()
}

// ── RECONCILE: DISCOVERY ─────────────────────────────────────────────

#[test]
fn newly_marked_heading_is_recorded_unlinked() {
    let mut db = StructureDb::default();

    assert!(reconcile_pages(&mut db, &[live("vii", &["par_3"])]));

    assert_eq!(db.headlines.len(), 1);
    assert_eq!(db.headlines[0].page, "vii");
    assert_eq!(db.headlines[0].block_id, "par_3");
    assert!(!db.headlines[0].is_linked);
    // Building the section tree is Codex's job, not ours.
    assert!(db.sections.is_empty());
}

#[test]
fn rows_follow_document_order_and_stay_grouped_by_page() {
    let mut db = StructureDb::default();
    reconcile_pages(&mut db, &[live("vii", &["par_1"])]);
    reconcile_pages(&mut db, &[live("viii", &["par_9"])]);
    // A second heading appears below the first on page vii.
    reconcile_pages(&mut db, &[live("vii", &["par_1", "par_4"])]);

    assert_eq!(ids(&db, "vii"), vec!["par_1", "par_4"]);
    assert_eq!(ids(&db, "viii"), vec!["par_9"]);
    // vii's new row was inserted next to its sibling, not appended after viii.
    let pages: Vec<&str> = db.headlines.iter().map(|r| r.page.as_str()).collect();
    assert_eq!(pages, vec!["vii", "vii", "viii"]);
}

#[test]
fn is_linked_is_derived_from_the_section_tree() {
    let mut db = StructureDb {
        sections: vec![section("ch1", Some(("vii", "par_3")))],
        headlines: vec![],
    };

    reconcile_pages(&mut db, &[live("vii", &["par_3", "par_8"])]);

    let linked = db.headlines.iter().find(|r| r.block_id == "par_3").unwrap();
    let loose = db.headlines.iter().find(|r| r.block_id == "par_8").unwrap();
    assert!(linked.is_linked, "a section points at par_3");
    assert!(!loose.is_linked, "nothing points at par_8");
}

#[test]
fn a_restored_heading_returns_to_its_document_position() {
    // Regression: dropping par_1 then re-adding it must not park it after
    // par_2. The "Captions" list is read in array order.
    let mut db = StructureDb::default();
    reconcile_pages(&mut db, &[live("vii", &["par_1", "par_2"])]);

    reconcile_pages(&mut db, &[live("vii", &["par_2"])]);
    assert_eq!(ids(&db, "vii"), vec!["par_2"]);

    reconcile_pages(&mut db, &[live("vii", &["par_1", "par_2"])]);
    assert_eq!(ids(&db, "vii"), vec!["par_1", "par_2"]);
}

#[test]
fn rebuilding_one_page_does_not_disturb_the_grouping_of_others() {
    let mut db = StructureDb::default();
    reconcile_pages(&mut db, &[live("a", &["par_1"])]);
    reconcile_pages(&mut db, &[live("b", &["par_2"])]);
    reconcile_pages(&mut db, &[live("c", &["par_3"])]);

    // Rebuild the middle page.
    reconcile_pages(&mut db, &[live("b", &["par_2", "par_9"])]);

    let pages: Vec<&str> = db.headlines.iter().map(|r| r.page.as_str()).collect();
    assert_eq!(pages, vec!["a", "b", "b", "c"]);
    assert_eq!(ids(&db, "b"), vec!["par_2", "par_9"]);
}

// ── RECONCILE: LOSS ──────────────────────────────────────────────────

#[test]
fn retyping_a_heading_to_paragraph_orphans_its_section() {
    let mut db = StructureDb {
        sections: vec![section("ch1", Some(("vii", "par_3")))],
        headlines: vec![row("vii", "par_3", true)],
    };

    // par_3 still exists but is no longer a headline kind, so it is absent
    // from the live set.
    assert!(reconcile_pages(&mut db, &[live("vii", &[])]));

    assert!(db.headlines.is_empty(), "the headline row is gone");
    assert_eq!(db.sections.len(), 1, "the section must survive");
    assert!(find(&db.sections, "ch1").unwrap().is_orphaned);
}

#[test]
fn block_removed_by_cascade_orphans_its_section() {
    // Deleting a block's last word deletes the line, the block, and possibly
    // the carea. From here that is indistinguishable from a retype, which is
    // exactly why reconciling beats diffing.
    let mut db = StructureDb {
        sections: vec![section("ch1", Some(("vii", "par_3")))],
        headlines: vec![row("vii", "par_3", true), row("vii", "par_7", false)],
    };

    reconcile_pages(&mut db, &[live("vii", &["par_7"])]);

    assert_eq!(ids(&db, "vii"), vec!["par_7"]);
    assert!(find(&db.sections, "ch1").unwrap().is_orphaned);
}

#[test]
fn merge_keeps_the_survivor_and_orphans_the_absorbed_section() {
    // merge_blocks discards every id but blocks[0].
    let mut db = StructureDb {
        sections: vec![
            section("ch1", Some(("vii", "par_3"))),
            section("ch2", Some(("vii", "par_4"))),
        ],
        headlines: vec![row("vii", "par_3", true), row("vii", "par_4", true)],
    };

    reconcile_pages(&mut db, &[live("vii", &["par_3"])]);

    assert_eq!(ids(&db, "vii"), vec!["par_3"]);
    assert!(!find(&db.sections, "ch1").unwrap().is_orphaned, "survivor");
    assert!(find(&db.sections, "ch2").unwrap().is_orphaned, "absorbed");
}

#[test]
fn nested_sections_are_orphaned_too() {
    let mut parent = section("ch1", Some(("vii", "par_1")));
    parent.subsections = vec![section("ch1.s1", Some(("vii", "par_3")))];
    let mut db = StructureDb {
        sections: vec![parent],
        headlines: vec![row("vii", "par_1", true), row("vii", "par_3", true)],
    };

    reconcile_pages(&mut db, &[live("vii", &["par_1"])]);

    assert!(!find(&db.sections, "ch1").unwrap().is_orphaned);
    assert!(find(&db.sections, "ch1.s1").unwrap().is_orphaned);
}

#[test]
fn other_pages_are_left_alone() {
    let mut db = StructureDb {
        sections: vec![section("ch9", Some(("xii", "par_3")))],
        headlines: vec![row("xii", "par_3", true)],
    };

    // Reconciling page vii must not touch page xii's rows or sections.
    assert!(!reconcile_pages(&mut db, &[live("vii", &[])]));

    assert_eq!(ids(&db, "xii"), vec!["par_3"]);
    assert!(!find(&db.sections, "ch9").unwrap().is_orphaned);
}

// ── RECONCILE: RECOVERY ──────────────────────────────────────────────

#[test]
fn restoring_a_heading_un_orphans_its_section() {
    let mut db = StructureDb {
        sections: vec![section("ch1", Some(("vii", "par_3")))],
        headlines: vec![row("vii", "par_3", true)],
    };

    reconcile_pages(&mut db, &[live("vii", &[])]);
    assert!(find(&db.sections, "ch1").unwrap().is_orphaned);

    // The user changes the block back to a heading.
    assert!(reconcile_pages(&mut db, &[live("vii", &["par_3"])]));

    assert!(!find(&db.sections, "ch1").unwrap().is_orphaned);
    assert_eq!(ids(&db, "vii"), vec!["par_3"]);
    assert!(db.headlines[0].is_linked, "the link is live again");
}

#[test]
fn reconcile_is_idempotent() {
    let mut db = StructureDb {
        sections: vec![section("ch1", Some(("vii", "par_3")))],
        headlines: vec![],
    };
    let state = [live("vii", &["par_3", "par_8"])];

    assert!(reconcile_pages(&mut db, &state), "first pass changes things");
    let snapshot = serde_json::to_string(&db).unwrap();

    assert!(
        !reconcile_pages(&mut db, &state),
        "second pass must report no change, so nothing is written to disk"
    );
    assert_eq!(snapshot, serde_json::to_string(&db).unwrap());
}

// ── PAGE NAMES ───────────────────────────────────────────────────────

#[test]
fn unnamed_pages_are_skipped() {
    let mut db = StructureDb {
        sections: vec![],
        headlines: vec![row("vii", "par_3", false)],
    };

    assert!(!reconcile_pages(&mut db, &[live("", &["par_1"])]));
    assert_eq!(db.headlines.len(), 1, "other pages untouched");
}

#[test]
fn renames_are_applied_simultaneously_not_chained() {
    // The permutation trap: p1 -> p2 while p2 -> p1. Rewriting row by row with
    // a chain of substitutions would collapse both onto one name.
    let mut db = StructureDb {
        sections: vec![
            section("a", Some(("p1", "par_1"))),
            section("b", Some(("p2", "par_2"))),
        ],
        headlines: vec![row("p1", "par_1", true), row("p2", "par_2", true)],
    };
    let renames = HashMap::from([
        ("p1".to_string(), "p2".to_string()),
        ("p2".to_string(), "p1".to_string()),
    ]);
    let live_names = HashSet::from(["p1".to_string(), "p2".to_string()]);

    assert!(remap_and_prune_pages(&mut db, &renames, &live_names));

    assert_eq!(ids(&db, "p2"), vec!["par_1"]);
    assert_eq!(ids(&db, "p1"), vec!["par_2"]);
    let a = find(&db.sections, "a").unwrap();
    let b = find(&db.sections, "b").unwrap();
    assert_eq!(a.headline.as_ref().unwrap().page, "p2");
    assert_eq!(b.headline.as_ref().unwrap().page, "p1");
}

#[test]
fn renames_are_keyed_on_scan_so_they_survive_renumbering() {
    let old = page_db(&[("s1.png", "1"), ("s2.png", "2")]);
    let new = page_db(&[("s1.png", "2"), ("s2.png", "3")]);

    let renames = page_renames(&old, &new);

    assert_eq!(renames.get("1").map(String::as_str), Some("2"));
    assert_eq!(renames.get("2").map(String::as_str), Some("3"));
}

#[test]
fn ambiguous_old_names_are_not_remapped() {
    // Two pages shared the name "?", so rows keyed "?" cannot be attributed.
    let old = page_db(&[("s1.png", "?"), ("s2.png", "?")]);
    let new = page_db(&[("s1.png", "5"), ("s2.png", "6")]);

    assert!(page_renames(&old, &new).is_empty());
}

#[test]
fn newly_named_page_is_not_treated_as_a_rename() {
    let old = page_db(&[("s1.png", "")]);
    let new = page_db(&[("s1.png", "vii")]);

    // Nothing to remap -- there were no rows under an empty name. The page is
    // instead re-read from hOCR by sync_page_db_change.
    assert!(page_renames(&old, &new).is_empty());
}

#[test]
fn deleting_a_page_prunes_rows_and_orphans_sections() {
    let mut db = StructureDb {
        sections: vec![section("ch1", Some(("vii", "par_3")))],
        headlines: vec![row("vii", "par_3", true), row("viii", "par_1", false)],
    };
    let live_names = HashSet::from(["viii".to_string()]);

    assert!(remap_and_prune_pages(&mut db, &HashMap::new(), &live_names));

    assert!(ids(&db, "vii").is_empty(), "rows for the dead page are pruned");
    assert_eq!(ids(&db, "viii"), vec!["par_1"], "survivors remain");
    assert_eq!(db.sections.len(), 1, "the section is kept");
    assert!(find(&db.sections, "ch1").unwrap().is_orphaned);
}

#[test]
fn duplicate_names_are_not_usable_keys() {
    let db = page_db(&[
        ("s1.png", "x"),
        ("s2.png", "x"),
        ("s3.png", "y"),
        ("s4.png", ""),
    ]);

    let usable = usable_page_names(&db);

    assert_eq!(usable, HashSet::from(["y".to_string()]));
    assert_eq!(page_name_for_stem(&db, "s1"), None, "ambiguous");
    assert_eq!(page_name_for_stem(&db, "s4"), None, "unnamed");
    assert_eq!(page_name_for_stem(&db, "s3"), Some("y".to_string()));
}

#[test]
fn page_name_lookup_accepts_a_stem_or_a_scan_filename() {
    let db = page_db(&[("side-014.jpg", "vii")]);

    assert_eq!(page_name_for_stem(&db, "side-014"), Some("vii".to_string()));
    assert_eq!(
        page_name_for_stem(&db, "side-014.jpg"),
        Some("vii".to_string())
    );
}

// ── EXTRACTION FROM hOCR ─────────────────────────────────────────────

const MIXED_PAGE: &str = r#"
<html><body>
  <div class='ocr_page' id='page_1' title='bbox 0 0 1000 1400'>
    <div class='ocr_carea' id='block_1_1' title="bbox 10 10 900 200">
      <h1 class='ocr_chapter' id='par_1_1' title="bbox 10 10 900 60">
        <span class='ocr_line' id='line_1_1' title="bbox 10 10 900 60">
          <span class='ocrx_word' id='word_1_1' title='bbox 10 10 200 60; x_wconf 95'>Chapter</span>
        </span>
      </h1>
      <p class='ocr_par' id='par_1_2' title="bbox 10 70 900 120">
        <span class='ocr_line' id='line_1_2' title="bbox 10 70 900 120">
          <span class='ocrx_word' id='word_1_2' title='bbox 10 70 200 120; x_wconf 95'>Body</span>
        </span>
      </p>
    </div>
    <div class='ocr_carea' id='block_1_2' title="bbox 10 210 900 400">
      <h2 class='ocr_section' id='par_1_3' title="bbox 10 210 900 260">
        <span class='ocr_line' id='line_1_3' title="bbox 10 210 900 260">
          <span class='ocrx_word' id='word_1_3' title='bbox 10 210 200 260; x_wconf 95'>Section</span>
        </span>
      </h2>
      <img class='ocr_photo' id='par_1_4' title="bbox 10 270 900 400"/>
    </div>
  </div>
</body></html>
"#;

#[test]
fn only_heading_kinds_are_extracted_in_document_order() {
    let parsed = crate::hocr_parser::parse(MIXED_PAGE, ParserConfig::default())
        .expect("fixture should parse");

    let found = page_headlines(&parsed, "vii");

    assert_eq!(found.page_name, "vii");
    assert_eq!(found.block_ids, vec!["par_1_1", "par_1_3"]);
}

#[test]
fn paragraph_and_image_blocks_add_no_rows() {
    // split_block hard-codes the new half to Paragraph, so a split heading
    // yields one heading plus one paragraph; only the heading is recorded.
    let parsed = crate::hocr_parser::parse(MIXED_PAGE, ParserConfig::default()).unwrap();
    let found = page_headlines(&parsed, "vii");

    let mut db = StructureDb::default();
    reconcile_pages(&mut db, std::slice::from_ref(&found));

    assert_eq!(ids(&db, "vii"), vec!["par_1_1", "par_1_3"]);
    for not_a_headline in ["par_1_2", "par_1_4"] {
        assert!(
            !db.headlines.iter().any(|r| r.block_id == not_a_headline),
            "{not_a_headline} must not become a headline"
        );
    }
}

#[test]
fn is_headline_matches_the_section_kind_set() {
    use crate::hocr_parser::HocrBlockKind as K;

    // The predicate and the conversion must agree, or reconcile would record
    // headlines Codex cannot turn into sections.
    for kind in [
        K::Paragraph,
        K::Part,
        K::Chapter,
        K::Section,
        K::Subsection,
        K::Subsubsection,
        K::Subsubsubsection,
        K::Subsubsubsubsection,
        K::Image,
        K::Table,
        K::List,
    ] {
        let as_section: Option<SectionKind> = kind.into();
        assert_eq!(
            kind.is_headline(),
            as_section.is_some(),
            "{kind:?} disagrees between is_headline() and SectionKind"
        );
    }
}

// ── DISK SAFETY ──────────────────────────────────────────────────────

/// Minimal scratch directory, removed on drop. Avoids a dev-dependency.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "aethergraph-sync-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Scratch(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_corrupt_structure_db_is_never_overwritten() {
    // The whole reason try_load_structure_db exists: load_structure_db maps a
    // parse failure to an empty database, so reconciling then saving would
    // silently replace the user's section tree with nothing.
    let scratch = Scratch::new("corrupt");
    let projects_dir = scratch.0.clone();
    let path = storage::structuredb_path(&projects_dir, "proj");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let garbage = b"{ this is not json";
    std::fs::write(&path, garbage).unwrap();

    apply(&projects_dir, "proj", |db| {
        db.headlines.push(row("vii", "par_3", false));
        true
    });

    assert_eq!(
        std::fs::read(&path).unwrap(),
        garbage,
        "the file must be byte-identical after a refused sync"
    );
}

#[test]
fn a_missing_structure_db_is_created() {
    let scratch = Scratch::new("missing");
    let projects_dir = scratch.0.clone();
    let path = storage::structuredb_path(&projects_dir, "proj");
    assert!(!path.exists());

    apply(&projects_dir, "proj", |db| {
        db.headlines.push(row("vii", "par_3", false));
        true
    });

    let written = storage::try_load_structure_db(&path).unwrap();
    assert_eq!(written.headlines.len(), 1);
}

#[test]
fn no_change_means_no_write() {
    let scratch = Scratch::new("nowrite");
    let projects_dir = scratch.0.clone();
    let path = storage::structuredb_path(&projects_dir, "proj");

    apply(&projects_dir, "proj", |_db| false);

    assert!(
        !path.exists(),
        "reporting no change must not touch the file at all"
    );
}

#[test]
fn atomic_save_leaves_no_temp_file_behind() {
    let scratch = Scratch::new("atomic");
    let path = scratch.0.join("sections").join("sectiondata.json");

    storage::save_structure_db_atomic(&path, &StructureDb::default()).unwrap();

    assert!(path.exists());
    let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| name.ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "stray temp files: {leftovers:?}");
}
