//! Integration tests for sisyphus-tools-core `wisdom` module's
//! pure data types and methods: `NotepadCategory` and `NotepadEntry`.

use sisyphus_tools_core::wisdom::{NotepadCategory, NotepadEntry};

// ---------------------------------------------------------------------------
// NotepadCategory: directory mapping
// ---------------------------------------------------------------------------

#[test]
fn notepad_category_as_dir_all_variants() {
    assert_eq!(NotepadCategory::Conventions.as_dir(), "conventions");
    assert_eq!(NotepadCategory::Successes.as_dir(), "successes");
    assert_eq!(NotepadCategory::Failures.as_dir(), "failures");
    assert_eq!(NotepadCategory::Patterns.as_dir(), "patterns");
    assert_eq!(NotepadCategory::Experience.as_dir(), "experience");
}

#[test]
fn notepad_category_label_all_variants() {
    assert_eq!(NotepadCategory::Conventions.label(), "conventions");
    assert_eq!(NotepadCategory::Successes.label(), "successes");
    assert_eq!(NotepadCategory::Failures.label(), "failures");
    assert_eq!(NotepadCategory::Patterns.label(), "patterns");
    assert_eq!(NotepadCategory::Experience.label(), "experience");
}

// ---------------------------------------------------------------------------
// NotepadCategory::parse
// ---------------------------------------------------------------------------

#[test]
fn notepad_category_parse_long_names() {
    assert_eq!(NotepadCategory::parse("conventions"), Some(NotepadCategory::Conventions));
    assert_eq!(NotepadCategory::parse("successes"), Some(NotepadCategory::Successes));
    assert_eq!(NotepadCategory::parse("failures"), Some(NotepadCategory::Failures));
    assert_eq!(NotepadCategory::parse("patterns"), Some(NotepadCategory::Patterns));
    assert_eq!(NotepadCategory::parse("experience"), Some(NotepadCategory::Experience));
}

#[test]
fn notepad_category_parse_short_aliases() {
    assert_eq!(NotepadCategory::parse("conv"), Some(NotepadCategory::Conventions));
    assert_eq!(NotepadCategory::parse("win"), Some(NotepadCategory::Successes));
    assert_eq!(NotepadCategory::parse("fail"), Some(NotepadCategory::Failures));
    assert_eq!(NotepadCategory::parse("pat"), Some(NotepadCategory::Patterns));
    assert_eq!(NotepadCategory::parse("exp"), Some(NotepadCategory::Experience));
}

#[test]
fn notepad_category_parse_case_insensitive() {
    assert_eq!(NotepadCategory::parse("CONVENTIONS"), Some(NotepadCategory::Conventions));
    assert_eq!(NotepadCategory::parse("Successes"), Some(NotepadCategory::Successes));
    assert_eq!(NotepadCategory::parse("PaTtErNs"), Some(NotepadCategory::Patterns));
}

#[test]
fn notepad_category_parse_knowledge_alias() {
    assert_eq!(
        NotepadCategory::parse("knowledge"),
        Some(NotepadCategory::Experience)
    );
}

#[test]
fn notepad_category_parse_invalid_returns_none() {
    assert_eq!(NotepadCategory::parse(""), None);
    assert_eq!(NotepadCategory::parse("nope"), None);
    assert_eq!(NotepadCategory::parse("foo"), None);
}

#[test]
fn notepad_category_all_constant() {
    assert_eq!(NotepadCategory::ALL.len(), 5);
    assert!(NotepadCategory::ALL.contains(&NotepadCategory::Conventions));
    assert!(NotepadCategory::ALL.contains(&NotepadCategory::Successes));
    assert!(NotepadCategory::ALL.contains(&NotepadCategory::Failures));
    assert!(NotepadCategory::ALL.contains(&NotepadCategory::Patterns));
    assert!(NotepadCategory::ALL.contains(&NotepadCategory::Experience));
}

// ---------------------------------------------------------------------------
// NotepadEntry: creation & fields
// ---------------------------------------------------------------------------

#[test]
fn notepad_entry_new_basic_fields() {
    let e = NotepadEntry::new(
        NotepadCategory::Conventions,
        "naming",
        "Use snake_case",
        vec!["style".into()],
    );
    assert_eq!(e.name, "naming");
    assert_eq!(e.category, NotepadCategory::Conventions);
    assert_eq!(e.content, "Use snake_case");
    assert_eq!(e.tags, vec!["style"]);
    assert!(e.timestamp > 0, "timestamp should be populated by SystemTime");
}

#[test]
fn notepad_entry_new_empty_tags() {
    let e = NotepadEntry::new(
        NotepadCategory::Successes,
        "first_test",
        "passed",
        vec![],
    );
    assert!(e.tags.is_empty());
    assert_eq!(e.content, "passed");
}

#[test]
fn notepad_entry_new_multiple_tags() {
    let e = NotepadEntry::new(
        NotepadCategory::Failures,
        "race",
        "fix it",
        vec!["bug".into(), "urgent".into(), "p1".into()],
    );
    assert_eq!(e.tags.len(), 3);
}

// ---------------------------------------------------------------------------
// NotepadEntry::to_markdown
// ---------------------------------------------------------------------------

#[test]
fn notepad_entry_to_markdown_with_tags() {
    let e = NotepadEntry {
        name: "x".into(),
        category: NotepadCategory::Patterns,
        timestamp: 1234567890,
        tags: vec!["a".into(), "b".into()],
        content: "body".into(),
    };
    let md = e.to_markdown();
    assert!(md.starts_with("---\n"));
    assert!(md.contains("name: x"));
    assert!(md.contains("category: patterns"));
    assert!(md.contains("timestamp: 1234567890"));
    assert!(md.contains("tags: a, b"));
    assert!(md.contains("body"));
}

#[test]
fn notepad_entry_to_markdown_no_tags() {
    let e = NotepadEntry {
        name: "x".into(),
        category: NotepadCategory::Conventions,
        timestamp: 100,
        tags: vec![],
        content: "hello".into(),
    };
    let md = e.to_markdown();
    assert!(md.contains("name: x"));
    assert!(!md.contains("tags:"));
    assert!(md.contains("hello"));
}

#[test]
fn notepad_entry_to_markdown_trims_trailing_whitespace() {
    let e = NotepadEntry {
        name: "x".into(),
        category: NotepadCategory::Experience,
        timestamp: 0,
        tags: vec![],
        content: "content with trailing\n\n".into(),
    };
    let md = e.to_markdown();
    assert!(!md.ends_with("\n\n\n"));
    assert!(md.contains("content with trailing"));
}

// ---------------------------------------------------------------------------
// NotepadEntry::from_markdown
// ---------------------------------------------------------------------------

#[test]
fn notepad_entry_from_markdown_full_front_matter() {
    let raw = "---\nname: rule\ncategory: conventions\ntimestamp: 9999\ntags: a, b, c\n---\n\nbody text\n";
    let e = NotepadEntry::from_markdown(
        "rule".into(),
        NotepadCategory::Conventions,
        raw,
    );
    assert_eq!(e.timestamp, 9999);
    assert_eq!(e.tags, vec!["a", "b", "c"]);
    assert!(e.content.contains("body text"));
}

#[test]
fn notepad_entry_from_markdown_no_front_matter() {
    let raw = "just content";
    let e = NotepadEntry::from_markdown(
        "n".into(),
        NotepadCategory::Successes,
        raw,
    );
    assert_eq!(e.timestamp, 0);
    assert!(e.tags.is_empty());
    assert_eq!(e.content, "just content");
}

#[test]
fn notepad_entry_from_markdown_partial_front_matter() {
    let raw = "---\ntimestamp: 42\n---\nbody";
    let e = NotepadEntry::from_markdown(
        "x".into(),
        NotepadCategory::Failures,
        raw,
    );
    assert_eq!(e.timestamp, 42);
    assert!(e.tags.is_empty());
    assert!(e.content.contains("body"));
}

#[test]
fn notepad_entry_from_markdown_empty_tags_in_front_matter() {
    let raw = "---\ntags: \n---\nx";
    let e = NotepadEntry::from_markdown("x".into(), NotepadCategory::Patterns, raw);
    assert!(e.tags.is_empty());
}

#[test]
fn notepad_entry_markdown_round_trip() {
    let e = NotepadEntry {
        name: "rt".into(),
        category: NotepadCategory::Experience,
        timestamp: 5555,
        tags: vec!["k1".into()],
        content: "round-trip content".into(),
    };
    let md = e.to_markdown();
    let back = NotepadEntry::from_markdown(e.name.clone(), e.category, &md);
    assert_eq!(back.timestamp, e.timestamp);
    assert_eq!(back.tags, e.tags);
    assert!(back.content.contains("round-trip content"));
}

#[test]
fn notepad_entry_from_markdown_strips_bom() {
    let raw = "\u{feff}---\ntimestamp: 1\n---\nbody";
    let e = NotepadEntry::from_markdown("n".into(), NotepadCategory::Conventions, raw);
    assert_eq!(e.timestamp, 1);
}
