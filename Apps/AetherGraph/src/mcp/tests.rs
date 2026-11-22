use super::*;
use crate::routes::projects::models::{Author, EditorPalette};
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
    list.projects.iter().map(|p| p.machine_name.as_str()).collect()
}

// ── MAPPING ──────────────────────────────────────────────────────────────────

#[test]
fn summaries_are_sorted_by_machine_name() {
    // read_projects yields fs::read_dir order, which is not stable; the tool must be.
    let list = summarise(vec![
        project("zeta-book", "Zeta"),
        project("alpha-book", "Alpha"),
        project("mid-book", "Mid"),
    ]);

    assert_eq!(codes(&list), vec!["alpha-book", "mid-book", "zeta-book"]);
}

#[test]
fn optional_fields_survive_as_none() {
    let list = summarise(vec![project("bare-book", "Bare")]);
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
fn published_date_is_formatted_as_iso() {
    // Must match the optional_date serde helper in models.rs, zero-padding included.
    let mut p = project("leviathan", "Leviathan");
    p.published = Some(time::Date::from_calendar_date(1651, Month::April, 5).unwrap());

    let list = summarise(vec![p]);

    assert_eq!(list.projects[0].published.as_deref(), Some("1651-04-05"));
}

#[test]
fn abbrev_is_carried_through_but_is_not_the_identifier() {
    let mut p = project("hobbes-leviathan", "Leviathan");
    p.abbrev = Some("LEV".to_string());

    let list = summarise(vec![p]);

    assert_eq!(list.projects[0].abbrev.as_deref(), Some("LEV"));
    assert_eq!(list.projects[0].machine_name, "hobbes-leviathan");
}

#[test]
fn authors_are_reduced_to_full_names() {
    let mut p = project("two-authors", "Two Authors");
    p.authors = vec![
        Author { full_name: "Thomas Hobbes".to_string(), abbrev: "TH".to_string() },
        Author { full_name: "John Locke".to_string(), abbrev: "JL".to_string() },
    ];

    let list = summarise(vec![p]);

    assert_eq!(list.projects[0].authors, vec!["Thomas Hobbes", "John Locke"]);
}

#[test]
fn empty_instance_yields_an_empty_list_not_an_error() {
    let list = summarise(vec![]);
    assert!(list.projects.is_empty());
}

// ── WIRE FORMAT ──────────────────────────────────────────────────────────────

#[test]
fn serialises_as_an_object_with_a_projects_array() {
    // MCP requires structuredContent to be a top-level object, hence ProjectList.
    let list = summarise(vec![project("alpha-book", "Alpha")]);
    let json = serde_json::to_value(&list).unwrap();

    assert!(json.is_object());
    assert!(json["projects"].is_array());
    assert_eq!(json["projects"][0]["machine_name"], "alpha-book");
}
