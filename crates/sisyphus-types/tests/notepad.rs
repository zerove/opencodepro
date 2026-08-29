//! Integration tests for sisyphus-types `SisyphusNotepad`.
//!
//! Exercises the public `SisyphusNotepad` / `NotepadSnapshot` API
//! which is the structural working-memory notepad used by the
//! cognitive loop. Provides write/read/append/snapshot/prune
//! operations over a persistent map of section id → content strings.

use sisyphus_types::SisyphusNotepad;

#[test]
fn notepad_starts_empty() {
    let n = SisyphusNotepad::default();
    assert!(n.sections.is_empty());
    assert!(n.history.is_empty());
    assert!(n.read_section("any").is_none());
}

#[test]
fn write_section_creates_new_section() {
    let mut n = SisyphusNotepad::default();
    n.write_section("plan".into(), "step 1".into());
    assert_eq!(n.read_section("plan"), Some(&"step 1".to_string()));
    assert_eq!(n.sections.len(), 1);
}

#[test]
fn write_section_overwrites_existing() {
    let mut n = SisyphusNotepad::default();
    n.write_section("plan".into(), "v1".into());
    n.write_section("plan".into(), "v2".into());
    assert_eq!(n.read_section("plan"), Some(&"v2".to_string()));
    assert_eq!(n.sections.len(), 1);
}

#[test]
fn write_section_supports_multiple_sections() {
    let mut n = SisyphusNotepad::default();
    n.write_section("a".into(), "alpha".into());
    n.write_section("b".into(), "beta".into());
    n.write_section("c".into(), "gamma".into());
    assert_eq!(n.sections.len(), 3);
    assert_eq!(n.read_section("b"), Some(&"beta".to_string()));
}

#[test]
fn read_section_unknown_returns_none() {
    let mut n = SisyphusNotepad::default();
    n.write_section("exists".into(), "x".into());
    assert!(n.read_section("doesnt").is_none());
}

#[test]
fn append_insight_to_new_section_initializes() {
    let mut n = SisyphusNotepad::default();
    n.append_insight("insights".into(), "first insight".into());
    let content = n.read_section("insights").expect("section");
    // Append format: "[<timestamp>] <insight>\n"
    assert!(content.contains("first insight"));
    assert!(content.contains('['));
    assert!(content.contains(']'));
}

#[test]
fn append_insight_appends_to_existing() {
    let mut n = SisyphusNotepad::default();
    n.append_insight("log".into(), "first".into());
    n.append_insight("log".into(), "second".into());
    let content = n.read_section("log").expect("section");
    assert!(content.contains("first"));
    assert!(content.contains("second"));
    // "second" should appear AFTER "first" in the file.
    let first_pos = content.find("first").unwrap();
    let second_pos = content.find("second").unwrap();
    assert!(first_pos < second_pos);
}

#[test]
fn append_insight_uses_timestamps() {
    let mut n = SisyphusNotepad::default();
    n.append_insight("t".into(), "a".into());
    n.append_insight("t".into(), "b".into());
    let content = n.read_section("t").expect("section");
    // Each line should start with a [N] timestamp.
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 2);
    for line in &lines {
        assert!(line.starts_with('['), "line {line:?} doesn't start with [");
        assert!(line.contains("] "), "line {line:?} missing ] separator");
    }
}

#[test]
fn take_snapshot_records_current_state() {
    let mut n = SisyphusNotepad::default();
    n.write_section("plan".into(), "do X".into());
    n.take_snapshot("node-1".into());
    assert_eq!(n.history.len(), 1);
    let snap = &n.history[0];
    assert_eq!(snap.trigger_node, "node-1");
    assert_eq!(snap.snapshot.get("plan").map(String::as_str), Some("do X"));
    assert!(snap.timestamp > 0);
}

#[test]
fn take_snapshot_does_not_modify_sections() {
    let mut n = SisyphusNotepad::default();
    n.write_section("a".into(), "1".into());
    n.take_snapshot("trigger".into());
    // Sections and history should both have data.
    assert_eq!(n.sections.len(), 1);
    assert_eq!(n.history.len(), 1);
}

#[test]
fn multiple_snapshots_accumulate() {
    let mut n = SisyphusNotepad::default();
    for i in 0..3 {
        n.write_section("a".into(), format!("v{i}"));
        n.take_snapshot(format!("n{i}"));
    }
    assert_eq!(n.history.len(), 3);
    assert_eq!(n.history[0].trigger_node, "n0");
    assert_eq!(n.history[2].trigger_node, "n2");
}

#[test]
fn prune_drops_oldest_lines_from_long_sections() {
    let mut n = SisyphusNotepad::default();
    let mut content = String::new();
    for i in 0..10 {
        content.push_str(&format!("line{i}\n"));
    }
    n.write_section("log".into(), content);
    n.prune(3);
    let pruned = n.read_section("log").expect("section");
    // Oldest 7 lines dropped, last 3 retained.
    assert!(pruned.contains("line7"));
    assert!(pruned.contains("line8"));
    assert!(pruned.contains("line9"));
    assert!(!pruned.contains("line0"));
    assert!(!pruned.contains("line6"));
}

#[test]
fn prune_leaves_short_sections_untouched() {
    let mut n = SisyphusNotepad::default();
    n.write_section("short".into(), "a\nb\nc\n".into());
    n.prune(10);
    assert_eq!(n.read_section("short"), Some(&"a\nb\nc\n".to_string()));
}

#[test]
fn prune_with_max_zero_empties_sections() {
    let mut n = SisyphusNotepad::default();
    n.write_section("a".into(), "1\n2\n3\n".into());
    n.prune(0);
    let pruned = n.read_section("a").expect("section");
    // max_insights=0 → 0 lines retained.
    assert!(pruned.is_empty() || pruned == "\n");
}
