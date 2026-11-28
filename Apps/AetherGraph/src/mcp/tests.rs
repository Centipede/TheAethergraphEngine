use super::dto::{self, ProjectList};
use super::features;
use super::pages::{self, ResolveError};
use crate::hocr_parser::{
    HocrBbox, HocrBlock, HocrBlockKind, HocrCarea, HocrLine, HocrPage, HocrWord,
};
use crate::routes::projects::models::{Author, EditorPalette, Page, Project};
use time::Month;

// ── HELPERS ──────────────────────────────────────────────────────────────────

/// `Project` derives neither `Default` nor `Clone`, so build one from scratch.
fn project(machine_name: &str, name: &str) -> Project {
    Project {
        name: name.to_string(),
        machine_name: machine_name.to_string(),
        abbrev: None,
        description: None,
        authors: vec![],
        published: None,
        ocr_language: None,
        flows: vec![],
        layouts: vec![],
        processing: None,
        editor_palette: EditorPalette::default(),
    }
}

fn codes(list: &ProjectList) -> Vec<&str> {
    list.projects
        .iter()
        .map(|p| p.machine_name.as_str())
        .collect()
}

fn page(scan: &str, name: &str) -> Page {
    Page {
        scan: scan.to_string(),
        name: name.to_string(),
        ..Page::default()
    }
}

fn word(text: &str, conf: i32) -> HocrWord {
    HocrWord {
        level: "word".to_string(),
        id: format!("word_{text}"),
        text: text.to_string(),
        wconf: conf,
        ..HocrWord::default()
    }
}

/// A line whose baseline sits `baseline_offset` below the bottom of its bbox.
fn line(id: &str, top: i32, bottom: i32, x_size: Option<f32>, words: Vec<HocrWord>) -> HocrLine {
    HocrLine {
        level: "line".to_string(),
        id: id.to_string(),
        bbox: HocrBbox::new(100, top, 900, bottom),
        baseline: Some((0.0, -5.0)),
        x_size,
        words,
        ..HocrLine::default()
    }
}

fn block(id: &str, left: i32, right: i32, lines: Vec<HocrLine>) -> HocrBlock {
    let top = lines.first().map(|l| l.bbox.top()).unwrap_or(0);
    let bottom = lines.last().map(|l| l.bbox.bottom()).unwrap_or(0);
    HocrBlock {
        level: "block".to_string(),
        id: id.to_string(),
        bbox: HocrBbox::new(left, top, right, bottom),
        lang: None,
        hints: Default::default(),
        metrics: None,
        kind: HocrBlockKind::Paragraph,
        lines,
    }
}

fn carea(id: &str, left: i32, right: i32, blocks: Vec<HocrBlock>) -> HocrCarea {
    let top = blocks.first().map(|b| b.bbox.top()).unwrap_or(0);
    let bottom = blocks.last().map(|b| b.bbox.bottom()).unwrap_or(0);
    HocrCarea {
        level: "carea".to_string(),
        id: id.to_string(),
        bbox: HocrBbox::new(left, top, right, bottom),
        flow: None,
        layout: None,
        blocks,
        unknowns: vec![],
    }
}

fn hocr_page(careas: Vec<HocrCarea>) -> HocrPage {
    HocrPage {
        level: "page".to_string(),
        page_id: "page_1".to_string(),
        bbox: HocrBbox::new(0, 0, 2000, 3000),
        careas,
        unknowns: vec![],
    }
}

/// A block of `n` lines whose baselines are `spacing` apart.
fn paragraph(id: &str, first_top: i32, n: usize, spacing: i32, x_size: f32) -> HocrBlock {
    let lines = (0..n)
        .map(|i| {
            let top = first_top + spacing * i as i32;
            line(
                &format!("{id}_l{i}"),
                top,
                top + 40,
                Some(x_size),
                vec![word("lorem", 90)],
            )
        })
        .collect();
    block(id, 100, 900, lines)
}

// ── PROJECT MAPPING ──────────────────────────────────────────────────────────

#[test]
fn summaries_are_sorted_by_machine_name() {
    // read_projects yields fs::read_dir order, which is not stable; the tool must be.
    let list = dto::summarise(vec![
        project("zeta-book", "Zeta"),
        project("alpha-book", "Alpha"),
        project("mid-book", "Mid"),
    ]);

    assert_eq!(codes(&list), vec!["alpha-book", "mid-book", "zeta-book"]);
}

#[test]
fn optional_fields_survive_as_none() {
    let list = dto::summarise(vec![project("bare-book", "Bare")]);
    let p = &list.projects[0];

    assert_eq!(p.machine_name, "bare-book");
    assert_eq!(p.name, "Bare");
    assert_eq!(p.abbrev, None);
    assert_eq!(p.description, None);
    assert_eq!(p.published, None);
    assert_eq!(p.ocr_language, None);
    assert!(p.authors.is_empty());
}

#[test]
fn blank_strings_are_reported_as_absent() {
    // Clearing a field in the UI leaves description = "" in project.toml; "no
    // description" and "empty description" should not look different to an agent.
    let mut p = project("blank-book", "Blank");
    p.description = Some("   ".to_string());
    p.abbrev = Some(String::new());

    let list = dto::summarise(vec![p]);

    assert_eq!(list.projects[0].description, None);
    assert_eq!(list.projects[0].abbrev, None);
}

#[test]
fn published_date_is_formatted_as_iso() {
    // Must match the optional_date serde helper in models.rs, zero-padding included.
    let mut p = project("leviathan", "Leviathan");
    p.published = Some(time::Date::from_calendar_date(1651, Month::April, 5).unwrap());

    let list = dto::summarise(vec![p]);

    assert_eq!(list.projects[0].published.as_deref(), Some("1651-04-05"));
}

#[test]
fn abbrev_is_carried_through_but_is_not_the_identifier() {
    let mut p = project("hobbes-leviathan", "Leviathan");
    p.abbrev = Some("LEV".to_string());

    let list = dto::summarise(vec![p]);

    assert_eq!(list.projects[0].abbrev.as_deref(), Some("LEV"));
    assert_eq!(list.projects[0].machine_name, "hobbes-leviathan");
}

#[test]
fn authors_are_reduced_to_full_names() {
    let mut p = project("two-authors", "Two Authors");
    p.authors = vec![
        Author {
            full_name: "Thomas Hobbes".to_string(),
            abbrev: "TH".to_string(),
        },
        Author {
            full_name: "John Locke".to_string(),
            abbrev: "JL".to_string(),
        },
    ];

    let list = dto::summarise(vec![p]);

    assert_eq!(
        list.projects[0].authors,
        vec!["Thomas Hobbes", "John Locke"]
    );
}

#[test]
fn empty_instance_yields_an_empty_list_not_an_error() {
    let list = dto::summarise(vec![]);
    assert!(list.projects.is_empty());
}

#[test]
fn serialises_as_an_object_with_a_projects_array() {
    // MCP requires structuredContent to be a top-level object, hence ProjectList.
    let list = dto::summarise(vec![project("alpha-book", "Alpha")]);
    let json = serde_json::to_value(&list).unwrap();

    assert!(json.is_object());
    assert!(json["projects"].is_array());
    assert_eq!(json["projects"][0]["machine_name"], "alpha-book");
}

// ── PAGE RESOLUTION ──────────────────────────────────────────────────────────

#[test]
fn page_resolves_by_scan_stem() {
    let db = vec![page("prolog-5.jpg", "vi"), page("side-011.jpg", "11")];
    let r = pages::resolve_page(&db, "prolog-5").unwrap();
    assert_eq!(r.stem, "prolog-5");
    assert_eq!(r.name, "vi");
}

#[test]
fn page_resolves_by_roman_printed_name() {
    // other-project's front matter is numbered ii..ix, so names are not numbers.
    let db = vec![page("prolog-5.jpg", "vi"), page("prolog-6.jpg", "vii")];
    let r = pages::resolve_page(&db, "vi").unwrap();
    assert_eq!(r.stem, "prolog-5");
    assert_eq!(r.scan, "prolog-5.jpg");
}

#[test]
fn printed_name_does_not_have_to_match_the_stem_number() {
    // test-project: printed page 11 is side-012.jpg. Resolving "11" by name must not be
    // mistaken for the stem side-011.
    let db = vec![page("side-011.jpg", "10"), page("side-012.jpg", "11")];

    assert_eq!(pages::resolve_page(&db, "11").unwrap().stem, "side-012");
    assert_eq!(pages::resolve_page(&db, "side-011").unwrap().name, "10");
}

#[test]
fn printed_name_lookup_ignores_case_and_padding() {
    let db = vec![page("prolog-5.jpg", "vi")];
    assert_eq!(pages::resolve_page(&db, " VI ").unwrap().stem, "prolog-5");
}

#[test]
fn a_stem_wins_over_a_printed_name() {
    // A page printed as "side-011" must not shadow the real side-011 scan.
    let db = vec![page("side-011.jpg", "7"), page("side-099.jpg", "side-011")];
    assert_eq!(pages::resolve_page(&db, "side-011").unwrap().name, "7");
}

#[test]
fn duplicate_printed_names_are_reported_as_ambiguous() {
    let db = vec![page("side-001.jpg", "1"), page("side-001b.jpg", "1")];
    match pages::resolve_page(&db, "1") {
        Err(ResolveError::Ambiguous { stems, .. }) => {
            assert_eq!(stems, vec!["side-001", "side-001b"]);
        }
        other => panic!("expected Ambiguous, got {other:?}"),
    }
}

#[test]
fn unknown_page_explains_both_addressing_schemes() {
    let db = vec![page("prolog-5.jpg", "vi")];
    let err = pages::resolve_page(&db, "nope").unwrap_err();
    let msg = err.message();
    assert!(msg.contains("nope"));
    assert!(msg.contains("stem"), "should mention stems: {msg}");
    assert!(msg.contains("printed"), "should mention printed name: {msg}");
}

// ── IMAGE SCALING ────────────────────────────────────────────────────────────

#[test]
fn scaling_preserves_aspect_ratio() {
    let (w, h) = pages::fit_to_long_edge(2000, 3000, 1400).unwrap();
    assert_eq!(h, 1400);
    assert_eq!(w, 933); // 2000 * 1400/3000
}

#[test]
fn scaling_never_upscales() {
    assert_eq!(pages::fit_to_long_edge(800, 600, 1400), None);
}

#[test]
fn scaling_is_capped() {
    // A request above MAX_SCALE is clamped, not honoured.
    let (_, h) = pages::fit_to_long_edge(3000, 9000, 100_000).unwrap();
    assert_eq!(h, pages::MAX_SCALE);
}

// ── MODE VALIDATION ──────────────────────────────────────────────────────────

#[test]
fn folio_modes_are_validated_against_real_routes() {
    assert_eq!(pages::validate_mode("edit").unwrap(), "edit");
    assert_eq!(pages::validate_mode(" Bridge ").unwrap(), "bridge");

    let err = pages::validate_mode("codex").unwrap_err();
    assert!(err.contains("codex"));
    assert!(err.contains("edit"), "should list valid modes: {err}");
}

// ── TEXT FEATURES ────────────────────────────────────────────────────────────

#[test]
fn all_caps_detects_the_heading_convention() {
    assert!(features::all_caps("II - NOBODY HOME"));
    assert!(features::all_caps("FORORD"));
    assert!(!features::all_caps("Defining politics"));
    assert!(!features::all_caps("What is Politics?"));
}

#[test]
fn digits_alone_are_not_all_caps() {
    // A bare folio number must not be read as a heading.
    assert!(!features::all_caps("1533"));
    assert!(!features::all_caps("  "));
}

#[test]
fn block_text_joins_words_across_lines() {
    let b = block(
        "b1",
        100,
        900,
        vec![
            line("l1", 100, 140, Some(48.0), vec![word("Motiver", 90), word("i", 88)]),
            line("l2", 150, 190, Some(48.0), vec![word("historie", 91)]),
        ],
    );
    assert_eq!(features::block_text(&b), "Motiver i historie");
}

#[test]
fn mean_confidence_averages_words() {
    let b = block(
        "b1",
        100,
        900,
        vec![line("l1", 100, 140, None, vec![word("a", 80), word("b", 90)])],
    );
    assert_eq!(features::mean_confidence(&b), Some(85.0));

    let empty = block("b2", 100, 900, vec![]);
    assert_eq!(features::mean_confidence(&empty), None);
}

// ── LEADING ──────────────────────────────────────────────────────────────────

#[test]
fn single_line_block_has_no_leading() {
    // The common case for headings, and the honest limitation of this measure.
    let b = paragraph("b1", 100, 1, 60, 48.0);
    assert_eq!(features::block_leading(&b), None);
}

#[test]
fn leading_is_the_median_baseline_gap_inside_a_block() {
    let b = paragraph("b1", 100, 4, 60, 48.0);
    assert_eq!(features::block_leading(&b), Some(60.0));
}

#[test]
fn body_leading_is_weighted_by_line_count() {
    // One two-line heading at 200px leading against a ten-line paragraph at 60px must
    // not drag the page reference upward: pooling raw gaps weights by line count, so the
    // paragraph's 9 gaps outvote the heading's 1.
    let page = hocr_page(vec![carea(
        "c1",
        100,
        900,
        vec![
            paragraph("heading", 100, 2, 200, 90.0),
            paragraph("body", 600, 10, 60, 48.0),
        ],
    )]);

    assert_eq!(features::body_leading(&page), Some(60.0));
}

#[test]
fn leading_ignores_non_positive_gaps() {
    // Lines grouped across a column break are not in reading order relative to each
    // other; such a pair carries no leading information and must be dropped, not
    // averaged in as a negative.
    let b = block(
        "b1",
        100,
        900,
        vec![
            line("l1", 1000, 1040, Some(48.0), vec![word("second", 90)]),
            line("l2", 100, 140, Some(48.0), vec![word("column", 90)]),
        ],
    );
    assert_eq!(features::block_leading(&b), None);
}

// ── GEOMETRY ─────────────────────────────────────────────────────────────────

#[test]
fn gap_above_is_none_for_the_first_block_in_a_carea() {
    let c = carea(
        "c1",
        100,
        900,
        vec![paragraph("b1", 100, 2, 60, 48.0), paragraph("b2", 400, 2, 60, 48.0)],
    );

    assert_eq!(features::gap_above(&c, 0, Some(60.0)), None);
    assert!(features::gap_above(&c, 1, Some(60.0)).is_some());
}

#[test]
fn gaps_never_cross_careas_so_columns_cannot_go_negative() {
    // The TOC case: column two starts high up the page again. If gap_above looked at the
    // previous carea, the first block of each later column would report a large negative
    // gap. Measured per carea, every gap stays sane.
    let page = hocr_page(vec![
        carea(
            "c1",
            100,
            700,
            vec![paragraph("a1", 200, 3, 60, 48.0), paragraph("a2", 600, 3, 60, 48.0)],
        ),
        carea(
            "c2",
            800,
            1400,
            vec![paragraph("b1", 200, 3, 60, 48.0), paragraph("b2", 600, 3, 60, 48.0)],
        ),
    ]);
    let stats = features::page_stats(&page);

    for c in &page.careas {
        for i in 0..c.blocks.len() {
            if let Some(gap) = features::gap_above(c, i, stats.body_leading) {
                assert!(gap >= 0.0, "negative gap in {} block {i}: {gap}", c.id);
            }
        }
    }
}

#[test]
fn centered_is_high_for_a_centred_block_and_low_for_a_flush_one() {
    let c = carea("c1", 0, 1000, vec![paragraph("b", 100, 1, 60, 48.0)]);

    let centred = block("mid", 400, 600, vec![line("l", 100, 140, None, vec![])]);
    let flush = block("left", 0, 200, vec![line("l", 100, 140, None, vec![])]);

    assert!(features::centered(&centred, &c).unwrap() > 0.99);
    assert!(features::centered(&flush, &c).unwrap() < 0.3);
}

#[test]
fn indent_is_measured_from_the_column_edge() {
    let page = hocr_page(vec![carea(
        "c1",
        100,
        900,
        vec![block(
            "b1",
            160,
            900,
            vec![line("l", 100, 140, Some(48.0), vec![word("x", 90)])],
        )],
    )]);
    let stats = features::page_stats(&page);
    let f = features::block_features(&page.careas[0], 0, &stats).unwrap();

    assert_eq!(f.indent, 60);
}

#[test]
fn a_display_heading_stands_out_on_size_ratios() {
    // Both measures separate large display type confidently; this is the case they
    // handle. They are NOT expected to separate near-body-size headings, which is why
    // the tool description tells the agent to confirm against the scan.
    let page = hocr_page(vec![carea(
        "c1",
        100,
        900,
        vec![
            block(
                "title",
                100,
                900,
                vec![line("t", 100, 300, Some(106.0), vec![word("INDHOLD", 92)])],
            ),
            paragraph("body", 400, 8, 60, 48.0),
        ],
    )]);
    let stats = features::page_stats(&page);

    let title = features::block_features(&page.careas[0], 0, &stats).unwrap();
    let body = features::block_features(&page.careas[0], 1, &stats).unwrap();

    assert!(title.rel_line_height.unwrap() > 1.8);
    assert!(body.rel_line_height.unwrap() < 1.2);
    assert!(title.rel_x_size.unwrap() > 1.8);
}

#[test]
fn missing_baseline_does_not_break_ratios() {
    let mut l = line("l", 100, 140, Some(48.0), vec![word("x", 90)]);
    l.baseline = None;
    let page = hocr_page(vec![carea("c1", 100, 900, vec![block("b", 100, 900, vec![l])])]);
    let stats = features::page_stats(&page);
    let f = features::block_features(&page.careas[0], 0, &stats).unwrap();

    assert_eq!(f.rel_ascent, None);
    assert!(f.rel_line_height.is_some());
}

// ── RUNNING HEADERS ──────────────────────────────────────────────────────────

fn page_with_header(header: &str, body: &str) -> HocrPage {
    hocr_page(vec![carea(
        "c1",
        100,
        900,
        vec![
            block(
                "hdr",
                100,
                900,
                vec![line("h", 60, 100, Some(40.0), vec![word(header, 90)])],
            ),
            block(
                "bdy",
                100,
                900,
                vec![line("b", 1400, 1440, Some(48.0), vec![word(body, 90)])],
            ),
        ],
    )])
}

#[test]
fn a_running_header_is_detected_across_pages() {
    let page = page_with_header("Ruslandshistorie", "Kiev");
    let neighbours = vec![
        page_with_header("Ruslandshistorie", "Novgorod"),
        page_with_header("Ruslandshistorie", "Moskva"),
    ];

    let repeats = features::repeating_block_ids(&page, &neighbours);

    assert!(repeats.contains("hdr"), "header should repeat");
    assert!(!repeats.contains("bdy"), "body text should not repeat");
}

#[test]
fn a_header_carrying_a_page_number_still_matches() {
    // Digits are stripped before comparison, because the folio number is the one part of
    // a running header guaranteed to differ page to page.
    let page = page_with_header("98 Ruslandshistorie", "Kiev");
    let neighbours = vec![page_with_header("99 Ruslandshistorie", "Novgorod")];

    assert!(features::repeating_block_ids(&page, &neighbours).contains("hdr"));
}

#[test]
fn a_bare_folio_number_is_not_flagged_as_a_header() {
    // Normalises to nothing, so it is skipped rather than matching every other number.
    let page = page_with_header("98", "Kiev");
    let neighbours = vec![page_with_header("99", "Novgorod")];

    assert!(features::repeating_block_ids(&page, &neighbours).is_empty());
}

#[test]
fn the_same_text_elsewhere_on_the_page_does_not_count_as_repeating() {
    // A chapter title in the body of one page and at the head of another is not a
    // running header; position has to agree too.
    let page = page_with_header("Kiev-Rusland", "x");
    let mut neighbour = page_with_header("zzz", "x");
    neighbour.careas[0].blocks[1].lines[0].words[0].text = "Kiev-Rusland".to_string();

    assert!(features::repeating_block_ids(&page, &[neighbour]).is_empty());
}

// ── PAGE VIEW ────────────────────────────────────────────────────────────────

#[test]
fn page_view_echoes_both_page_names_and_keeps_scan_coordinates() {
    let page = hocr_page(vec![carea(
        "c1",
        100,
        900,
        vec![paragraph("b1", 200, 3, 60, 48.0)],
    )]);
    let page_ref = pages::PageRef {
        stem: "prolog-5".to_string(),
        name: "vi".to_string(),
        scan: "prolog-5.jpg".to_string(),
    };

    let view = dto::page_view("other-project", &page_ref, false, &page, &[]);

    assert_eq!(view.stem, "prolog-5");
    assert_eq!(view.name, "vi");
    assert_eq!(view.page_bbox, [0, 0, 2000, 3000]);

    let b = &view.careas[0].blocks[0];
    assert_eq!(b.kind, "paragraph");
    assert_eq!(b.line_count, 3);
    assert_eq!(b.bbox, [100, 200, 900, 360]);
}

#[test]
fn every_block_kind_round_trips_through_its_json_name() {
    // The agent sets types by these names, so json_name and from_json_name must agree
    // for all of them.
    for kind in HocrBlockKind::ALL {
        let name = kind.json_name();
        assert_eq!(
            HocrBlockKind::from_json_name(name),
            Some(kind),
            "round trip failed for {name}"
        );
    }
}
