use super::*;

#[test]
fn test_auto_detection_reset_and_assigned() {
    let mut block = HocrBlock {
        id: "b1".to_string(),
        level: "block".to_string(),
        kind: HocrBlockKind::Paragraph,
        lang: None,
        bbox: HocrBbox([100, 100, 500, 200]),
        hints: HocrBlockHints {
            test_x_indent: Evidence::Determined(true),
            test_x_dedent: Evidence::Suggested(true),
            test_hyphenation: Evidence::Determined(false),
            test_y_advance: Evidence::Determined(true),
            test_y_reverse: Evidence::Determined(false),
            test_preceding_terminal: Evidence::Untested,
            test_following_terminal: Evidence::Untested,
            break_from_preceding: Evidence::Assigned(true), // Should be preserved
            break_from_following: Evidence::Suggested(false),
            ..Default::default()
        },
        metrics: None,
        lines: vec![],
    };

    let thresholds = DetectionThresholds {
        x_indent: DetectionRange { certainly_true: 20, suggested_true: 15, suggested_false: 10, certainly_false: 5 },
        x_dedent: DetectionRange { certainly_true: 20, suggested_true: 15, suggested_false: 10, certainly_false: 5 },
        y_advance: DetectionRange { certainly_true: 20, suggested_true: 15, suggested_false: 10, certainly_false: 5 },
        use_x_indent: false, // Should result in Untested
        use_x_dedent: false,
        use_hyphenation: false,
        use_y_advance: false,
    };

    block.update_metrics(None, None, "page1", HocrPath::Block { carea: 0, block: 0 });
    block.auto_bridge(None, None, "page1", HocrPath::Block { carea: 0, block: 0 }, &thresholds);

    assert_eq!(block.hints.test_x_indent, Evidence::Untested);
    assert_eq!(block.hints.test_x_dedent, Evidence::Untested);
    assert_eq!(block.hints.test_hyphenation, Evidence::Untested);
    assert_eq!(block.hints.test_y_advance, Evidence::Untested);
    assert_eq!(block.hints.test_y_reverse, Evidence::Untested);
    assert_eq!(block.hints.break_from_preceding, Evidence::Assigned(true)); // Preserved
    assert_eq!(block.hints.break_from_following, Evidence::Untested); // Recalculated from Untested inputs -> Untested
}

#[test]
fn test_derive_evidence_assigned_and_untested() {
    // Test Assigned values
    assert_eq!(operations::derive_evidence(&[Evidence::Assigned(true), Evidence::Determined(false)]), Evidence::Error);
    assert_eq!(operations::derive_evidence(&[Evidence::Assigned(true), Evidence::Untested]), Evidence::Determined(true));
    assert_eq!(operations::derive_evidence(&[Evidence::Assigned(false), Evidence::Suggested(true)]), Evidence::Determined(false));

    // Test All Untested
    assert_eq!(operations::derive_evidence(&[Evidence::Untested, Evidence::Untested]), Evidence::Untested);

    // Test Mixed Untested and Evidence
    assert_eq!(operations::derive_evidence(&[Evidence::Untested, Evidence::Suggested(true)]), Evidence::Suggested(true));
}

#[test]
fn test_terminal_block_detection() {
    let header = HocrBlock {
        id: "h1".to_string(),
        level: "block".to_string(),
        kind: HocrBlockKind::Chapter,
        lang: None,
        bbox: HocrBbox([100, 100, 500, 150]),
        hints: HocrBlockHints::default(),
        metrics: None,
        lines: vec![],
    };

    let mut paragraph = HocrBlock {
        id: "p1".to_string(),
        level: "block".to_string(),
        kind: HocrBlockKind::Paragraph,
        lang: None,
        bbox: HocrBbox([100, 160, 500, 250]),
        hints: HocrBlockHints::default(),
        metrics: None,
        lines: vec![],
    };

    let thresholds = DetectionThresholds::default();

    paragraph.update_metrics(
        Some((&header, "page1", HocrPath::Block { carea: 0, block: 0 })),
        None,
        "page1",
        HocrPath::Block { carea: 0, block: 1 },
    );
    paragraph.auto_bridge(
        Some((&header, "page1", HocrPath::Block { carea: 0, block: 0 })),
        None,
        "page1",
        HocrPath::Block { carea: 0, block: 1 },
        &thresholds
    );

    assert_eq!(paragraph.hints.test_preceding_terminal, Evidence::Determined(true));
    // break_from_preceding is derived from test_x_indent, test_y_advance, and test_preceding_terminal.
    // Since lines is empty, x_indent is 0. With default thresholds, x_indent_min=5, so x_indent is 0 < 5 -> Determined(false).
    // y_advance is (160 - 150) = 10. Default y_advance_min=0, y_advance_max=0, so y_advance is 10 >= 0 -> Determined(true).
    // test_preceding_terminal is Determined(true).
    // So derive_evidence([Determined(false), Determined(true), Determined(true)]) -> Evidence::Error because of conflicting Determinations.
    // Wait, let's check default thresholds again in models.rs.
    // y_advance_min: 0, y_advance_max: 0.
    // val >= thresholds.y_advance_max { Evidence::Determined(true) } -> 10 >= 0 is true.
    // val < thresholds.x_indent_min { Evidence::Determined(false) } -> 0 < 5 is true.
    // test_preceding_terminal is Determined(true).
    // So [Determined(false), Determined(true), Determined(true)] -> Error.
    
    // Actually, I should probably check that it correctly identified the terminal block.
    // To make break_from_preceding Determined(true), I should make others Untested or Determined(true).
    
    let mut thresholds_only_terminal = DetectionThresholds::default();
    thresholds_only_terminal.use_x_indent = false;
    thresholds_only_terminal.use_y_advance = false;

    paragraph.hints = HocrBlockHints::default(); // Reset hints
    paragraph.update_metrics(
        Some((&header, "page1", HocrPath::Block { carea: 0, block: 0 })),
        None,
        "page1",
        HocrPath::Block { carea: 0, block: 1 },
    );
    paragraph.auto_bridge(
        Some((&header, "page1", HocrPath::Block { carea: 0, block: 0 })),
        None,
        "page1",
        HocrPath::Block { carea: 0, block: 1 },
        &thresholds_only_terminal
    );

    assert_eq!(paragraph.hints.test_preceding_terminal, Evidence::Determined(true));
    assert_eq!(paragraph.hints.break_from_preceding, Evidence::Determined(true));
}

#[test]
fn test_terminal_hints_persistence() {
    let block = HocrBlock {
        id: "b1".to_string(),
        level: "block".to_string(),
        kind: HocrBlockKind::Paragraph,
        lang: None,
        bbox: HocrBbox([100, 100, 500, 200]),
        hints: HocrBlockHints {
            test_preceding_terminal: Evidence::Determined(true),
            test_following_terminal: Evidence::Determined(false),
            ..HocrBlockHints::default()
        },
        metrics: None,
        lines: vec![],
    };

    let html = block.to_hocr_html();
    assert!(html.contains("test_preceding_terminal determined_true"));
    assert!(html.contains("test_following_terminal determined_false"));

    // To test parsing, we need a full page or a way to parse just a block.
    // parser::parse takes a full HTML string.
    let full_html = format!(
        r#"<!DOCTYPE html><html><body><div class="ocr_page" id="page_1" title="bbox 0 0 1000 1000"><div class="ocr_carea" id="carea_1" title="bbox 0 0 1000 1000">{}</div></div></body></html>"#,
        html
    );

    let parsed_page = parser::parse(&full_html, parser::ParserConfig::default()).unwrap();
    let parsed_block = &parsed_page.careas[0].blocks[0];

    assert_eq!(parsed_block.hints.test_preceding_terminal, Evidence::Determined(true));
    assert_eq!(parsed_block.hints.test_following_terminal, Evidence::Determined(false));
}

#[test]
fn test_x_indent_dedent_omitted_for_single_line() {
    let line = HocrLine {
        id: "l1".to_string(),
        level: "line".to_string(),
        bbox: HocrBbox([110, 110, 490, 130]),
        lang: None,
        words: vec![],
        baseline: None,
        x_size: None,
        x_descenders: None,
        x_ascenders: None,
    };
    let mut block = HocrBlock {
        id: "b1".to_string(),
        level: "block".to_string(),
        kind: HocrBlockKind::Paragraph,
        lang: None,
        bbox: HocrBbox([100, 100, 500, 140]),
        hints: HocrBlockHints::default(),
        metrics: None,
        lines: vec![line],
    };

    let thresholds = DetectionThresholds {
        use_x_indent: true,
        use_x_dedent: true,
        x_indent: DetectionRange { certainly_true: 15, suggested_true: 10, suggested_false: 5, certainly_false: 2 },
        x_dedent: DetectionRange { certainly_true: 15, suggested_true: 10, suggested_false: 5, certainly_false: 2 },
        ..Default::default()
    };

    block.update_metrics(None, None, "page1", HocrPath::Block { carea: 0, block: 0 });
    block.auto_bridge(None, None, "page1", HocrPath::Block { carea: 0, block: 0 }, &thresholds);

    assert_eq!(block.hints.test_x_indent, Evidence::Untested);
    assert_eq!(block.hints.test_x_dedent, Evidence::Untested);
}

#[test]
fn test_x_indent_dedent_omitted_for_zero_lines() {
    let mut block = HocrBlock {
        id: "b1".to_string(),
        level: "block".to_string(),
        kind: HocrBlockKind::Paragraph,
        lang: None,
        bbox: HocrBbox([100, 100, 500, 140]),
        hints: HocrBlockHints::default(),
        metrics: None,
        lines: vec![],
    };

    let thresholds = DetectionThresholds {
        use_x_indent: true,
        use_x_dedent: true,
        ..Default::default()
    };

    block.update_metrics(None, None, "page1", HocrPath::Block { carea: 0, block: 0 });
    block.auto_bridge(None, None, "page1", HocrPath::Block { carea: 0, block: 0 }, &thresholds);

    assert_eq!(block.hints.test_x_indent, Evidence::Untested);
    assert_eq!(block.hints.test_x_dedent, Evidence::Untested);
}

#[test]
fn test_x_indent_dedent_included_for_multiple_lines() {
    let line1 = HocrLine {
        id: "l1".to_string(),
        level: "line".to_string(),
        bbox: HocrBbox([120, 110, 490, 130]), // x_indent = 20
        lang: None,
        words: vec![],
        baseline: None,
        x_size: None,
        x_descenders: None,
        x_ascenders: None,
    };
    let line2 = HocrLine {
        id: "l2".to_string(),
        level: "line".to_string(),
        bbox: HocrBbox([100, 140, 480, 160]), // x_dedent = 20
        lang: None,
        words: vec![],
        baseline: None,
        x_size: None,
        x_descenders: None,
        x_ascenders: None,
    };
    let mut block = HocrBlock {
        id: "b1".to_string(),
        level: "block".to_string(),
        kind: HocrBlockKind::Paragraph,
        lang: None,
        bbox: HocrBbox([100, 100, 500, 170]),
        hints: HocrBlockHints::default(),
        metrics: None,
        lines: vec![line1, line2],
    };

    let thresholds = DetectionThresholds {
        use_x_indent: true,
        use_x_dedent: true,
        x_indent: DetectionRange { certainly_true: 15, suggested_true: 10, suggested_false: 5, certainly_false: 2 },
        x_dedent: DetectionRange { certainly_true: 15, suggested_true: 10, suggested_false: 5, certainly_false: 2 },
        ..Default::default()
    };

    block.update_metrics(None, None, "page1", HocrPath::Block { carea: 0, block: 0 });
    block.auto_bridge(None, None, "page1", HocrPath::Block { carea: 0, block: 0 }, &thresholds);

    assert_eq!(block.hints.test_x_indent, Evidence::Determined(true)); // 20 >= 15
    assert_eq!(block.hints.test_x_dedent, Evidence::Determined(true)); // 20 >= 15
}

#[test]
fn test_assigned_preserved_for_single_line() {
    let line = HocrLine {
        id: "l1".to_string(),
        level: "line".to_string(),
        bbox: HocrBbox([110, 110, 490, 130]),
        lang: None,
        words: vec![],
        baseline: None,
        x_size: None,
        x_descenders: None,
        x_ascenders: None,
    };
    let mut block = HocrBlock {
        id: "b1".to_string(),
        level: "block".to_string(),
        kind: HocrBlockKind::Paragraph,
        lang: None,
        bbox: HocrBbox([100, 100, 500, 140]),
        hints: HocrBlockHints {
            test_x_indent: Evidence::Assigned(true),
            test_x_dedent: Evidence::Assigned(false),
            ..HocrBlockHints::default()
        },
        metrics: None,
        lines: vec![line],
    };

    let thresholds = DetectionThresholds {
        use_x_indent: true,
        use_x_dedent: true,
        ..Default::default()
    };

    block.update_metrics(None, None, "page1", HocrPath::Block { carea: 0, block: 0 });
    block.auto_bridge(None, None, "page1", HocrPath::Block { carea: 0, block: 0 }, &thresholds);

    assert_eq!(block.hints.test_x_indent, Evidence::Assigned(true));
    assert_eq!(block.hints.test_x_dedent, Evidence::Assigned(false));
}

#[test]
fn test_four_value_detection_logic() {
    let mut block = HocrBlock {
        id: "b1".to_string(),
        level: "block".to_string(),
        kind: HocrBlockKind::Paragraph,
        lang: None,
        bbox: HocrBbox([100, 100, 500, 300]),
        hints: HocrBlockHints::default(),
        metrics: Some(HocrBlockMetrics {
            x_indent: 10,
            x_dedent: 0,
            y_advance: Some(2),
            y_reverse: None,
            has_final_hyphen: false,
        }),
        lines: vec![
            HocrLine { id: "l1".to_string(), level: "line".to_string(), bbox: HocrBbox([110, 110, 490, 130]), words: vec![], ..Default::default() },
            HocrLine { id: "l2".to_string(), level: "line".to_string(), bbox: HocrBbox([100, 140, 490, 160]), words: vec![], ..Default::default() },
        ],
    };

    let thresholds = DetectionThresholds {
        x_indent: DetectionRange {
            certainly_true: 15,
            suggested_true: 8,
            suggested_false: 4,
            certainly_false: 1,
        },
        y_advance: DetectionRange {
            certainly_true: 10,
            suggested_true: 5,
            suggested_false: 3,
            certainly_false: 0,
        },
        ..DetectionThresholds::default()
    };

    block.auto_bridge(None, None, "page1", HocrPath::Block { carea: 0, block: 0 }, &thresholds);

    // x_indent = 10. certainly_true: 15, suggested_true: 8. 10 >= 8 -> Suggested(true)
    assert_eq!(block.hints.test_x_indent, Evidence::Suggested(true));

    // y_advance = 2. certainly_true: 10, suggested_true: 5, suggested_false: 3. 2 <= 3 -> Suggested(false)
    assert_eq!(block.hints.test_y_advance, Evidence::Suggested(false));
}

// ── Cross-boundary conflicts ─────────────────────────────────────────

/// A provider holding pages in memory, so the cross-boundary pass can be
/// exercised without touching the filesystem.
struct MapProvider {
    pages: Vec<(String, HocrPage)>,
}

impl crate::hocr_parser::navigation::HocrPageProvider for MapProvider {
    fn get_page(&self, page_id: &str) -> Option<HocrPage> {
        self.pages
            .iter()
            .find(|(id, _)| id == page_id)
            .map(|(_, page)| page.clone())
    }

    fn get_page_order(&self) -> Vec<String> {
        self.pages.iter().map(|(id, _)| id.clone()).collect()
    }
}

fn conflict_block(id: &str, top: i32, from_preceding: Evidence, from_following: Evidence) -> HocrBlock {
    HocrBlock {
        id: id.to_string(),
        level: "block".to_string(),
        kind: HocrBlockKind::Paragraph,
        lang: None,
        bbox: HocrBbox([100, top, 500, top + 80]),
        hints: HocrBlockHints {
            break_from_preceding: from_preceding,
            break_from_following: from_following,
            ..Default::default()
        },
        metrics: None,
        lines: vec![],
    }
}

fn page_with(page_id: &str, careas: Vec<HocrCarea>) -> HocrPage {
    HocrPage {
        level: "page".to_string(),
        page_id: page_id.to_string(),
        bbox: HocrBbox::new(0, 0, 1000, 1000),
        careas,
        unknowns: vec![],
    }
}

fn carea_with(id: &str, flow: Option<&str>, blocks: Vec<HocrBlock>) -> HocrCarea {
    HocrCarea {
        level: "carea".to_string(),
        id: id.to_string(),
        bbox: HocrBbox::new(100, 0, 500, 1000),
        flow: flow.map(str::to_string),
        layout: None,
        blocks,
        unknowns: vec![],
    }
}

#[test]
fn test_boundary_conflict_truth_table() {
    use operations::boundary_conflict;

    // An unanalysed neighbour gives nothing to compare against.
    let a = conflict_block("a", 0, Evidence::Untested, Evidence::Untested);
    let b = conflict_block("b", 100, Evidence::Determined(true), Evidence::Untested);
    assert_eq!(boundary_conflict(&a, &b), Evidence::Untested);
    assert_eq!(boundary_conflict(&b, &a), Evidence::Untested);

    // Sides agree: both say the paragraph breaks at this edge.
    let a = conflict_block("a", 0, Evidence::Untested, Evidence::Determined(true));
    let b = conflict_block("b", 100, Evidence::Determined(true), Evidence::Untested);
    assert_eq!(boundary_conflict(&a, &b), Evidence::Determined(false));

    // Sides agree: both say it continues.
    let a = conflict_block("a", 0, Evidence::Untested, Evidence::Suggested(false));
    let b = conflict_block("b", 100, Evidence::Suggested(false), Evidence::Untested);
    assert_eq!(boundary_conflict(&a, &b), Evidence::Determined(false));

    // Hard conflict: one side is Determined.
    let a = conflict_block("a", 0, Evidence::Untested, Evidence::Determined(true));
    let b = conflict_block("b", 100, Evidence::Suggested(false), Evidence::Untested);
    assert_eq!(boundary_conflict(&a, &b), Evidence::Determined(true));

    // Hard conflict: one side was assigned by the user.
    let a = conflict_block("a", 0, Evidence::Untested, Evidence::Assigned(false));
    let b = conflict_block("b", 100, Evidence::Suggested(true), Evidence::Untested);
    assert_eq!(boundary_conflict(&a, &b), Evidence::Determined(true));

    // Soft conflict: the disagreement rests on suggestions only.
    let a = conflict_block("a", 0, Evidence::Untested, Evidence::Suggested(true));
    let b = conflict_block("b", 100, Evidence::Suggested(false), Evidence::Untested);
    assert_eq!(boundary_conflict(&a, &b), Evidence::Suggested(true));

    // A block that already contradicts itself cannot vouch for its edges.
    let a = conflict_block("a", 0, Evidence::Untested, Evidence::Error);
    let b = conflict_block("b", 100, Evidence::Determined(false), Evidence::Untested);
    assert_eq!(boundary_conflict(&a, &b), Evidence::Determined(true));

    // An undecided side yields no verdict either way.
    let a = conflict_block("a", 0, Evidence::Untested, Evidence::Undetermined);
    let b = conflict_block("b", 100, Evidence::Determined(true), Evidence::Untested);
    assert_eq!(boundary_conflict(&a, &b), Evidence::Undetermined);
}

#[test]
fn test_annotate_cross_boundaries_marks_both_blocks() {
    // b1 says it continues into b2; b2 says it breaks from b1.
    let b1 = conflict_block("b1", 0, Evidence::Untested, Evidence::Determined(false));
    let b2 = conflict_block("b2", 100, Evidence::Determined(true), Evidence::Untested);
    let mut page = page_with("page1", vec![carea_with("c1", Some("main"), vec![b1, b2])]);

    let provider = MapProvider { pages: vec![("page1".to_string(), page.clone())] };
    page.annotate_cross_boundaries(&provider, "page1");

    let blocks = &page.careas[0].blocks;
    // The shared edge is flagged on both sides of it.
    assert_eq!(blocks[0].hints.conflict_with_following, Evidence::Determined(true));
    assert_eq!(blocks[1].hints.conflict_with_preceding, Evidence::Determined(true));
    // The outer edges have no neighbour in this flow.
    assert_eq!(blocks[0].hints.conflict_with_preceding, Evidence::Untested);
    assert_eq!(blocks[1].hints.conflict_with_following, Evidence::Untested);
}

#[test]
fn test_annotate_cross_boundaries_respects_flows() {
    // Identical disagreement, but the two blocks sit in different flows and so
    // never share a boundary.
    let b1 = conflict_block("b1", 0, Evidence::Untested, Evidence::Determined(false));
    let b2 = conflict_block("b2", 100, Evidence::Determined(true), Evidence::Untested);
    let mut page = page_with(
        "page1",
        vec![
            carea_with("c1", Some("main"), vec![b1]),
            carea_with("c2", Some("footnote"), vec![b2]),
        ],
    );

    let provider = MapProvider { pages: vec![("page1".to_string(), page.clone())] };
    page.annotate_cross_boundaries(&provider, "page1");

    assert_eq!(page.careas[0].blocks[0].hints.conflict_with_following, Evidence::Untested);
    assert_eq!(page.careas[1].blocks[0].hints.conflict_with_preceding, Evidence::Untested);
}

#[test]
fn test_annotate_cross_boundaries_spans_pages() {
    // The flow continues onto page2, so the last block of page1 is compared
    // against the first block of page2.
    let p1_block = conflict_block("b1", 0, Evidence::Untested, Evidence::Determined(false));
    let p2_block = conflict_block("b2", 0, Evidence::Assigned(true), Evidence::Untested);

    let mut page1 = page_with("page1", vec![carea_with("c1", Some("main"), vec![p1_block])]);
    let page2 = page_with("page2", vec![carea_with("c2", Some("main"), vec![p2_block])]);

    let provider = MapProvider {
        pages: vec![
            ("page1".to_string(), page1.clone()),
            ("page2".to_string(), page2.clone()),
        ],
    };
    page1.annotate_cross_boundaries(&provider, "page1");

    assert_eq!(
        page1.careas[0].blocks[0].hints.conflict_with_following,
        Evidence::Determined(true)
    );
}

#[test]
fn test_auto_bridge_clears_stale_conflict() {
    // auto_bridge sees one block at a time, so it must never leave a verdict
    // that was derived from a neighbour it cannot see.
    let mut block = conflict_block("b1", 0, Evidence::Untested, Evidence::Untested);
    block.hints.conflict_with_preceding = Evidence::Determined(true);
    block.hints.conflict_with_following = Evidence::Suggested(true);

    block.auto_bridge(None, None, "page1", HocrPath::Block { carea: 0, block: 0 }, &DetectionThresholds::default());

    assert_eq!(block.hints.conflict_with_preceding, Evidence::Untested);
    assert_eq!(block.hints.conflict_with_following, Evidence::Untested);
}

#[test]
fn test_hyphenation_feeds_break_from_following() {
    let hyphenated = |text: &str| HocrBlock {
        id: "b1".to_string(),
        level: "block".to_string(),
        kind: HocrBlockKind::Paragraph,
        lang: None,
        bbox: HocrBbox([100, 100, 500, 200]),
        hints: HocrBlockHints::default(),
        metrics: None,
        lines: vec![HocrLine {
            level: "line".to_string(),
            id: "l1".to_string(),
            bbox: HocrBbox([100, 100, 500, 200]),
            lang: None,
            words: vec![HocrWord {
                level: "word".to_string(),
                id: "w1".to_string(),
                bbox: HocrBbox([100, 100, 500, 200]),
                lang: None,
                text: text.to_string(),
                wconf: 95,
                dropcap: None,
            }],
            baseline: None,
            x_size: None,
            x_descenders: None,
            x_ascenders: None,
        }],
    };

    // Only hyphenation is in play; every geometric test is switched off.
    let thresholds = DetectionThresholds {
        use_x_indent: false,
        use_x_dedent: false,
        use_y_advance: false,
        use_hyphenation: true,
        ..DetectionThresholds::default()
    };
    let path = HocrPath::Block { carea: 0, block: 0 };

    // A trailing hyphen means the paragraph carries on: it does NOT break.
    let mut block = hyphenated("contin-");
    block.update_metrics(None, None, "page1", path);
    block.auto_bridge(None, None, "page1", path, &thresholds);
    assert_eq!(block.hints.test_hyphenation, Evidence::Suggested(true));
    assert_eq!(block.hints.break_from_following, Evidence::Suggested(false));

    // No hyphen is weak evidence the other way.
    let mut block = hyphenated("end.");
    block.update_metrics(None, None, "page1", path);
    block.auto_bridge(None, None, "page1", path, &thresholds);
    assert_eq!(block.hints.test_hyphenation, Evidence::Suggested(false));
    assert_eq!(block.hints.break_from_following, Evidence::Suggested(true));
}
