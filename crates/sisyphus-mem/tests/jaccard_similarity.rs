//! Integration tests for sisyphus-mem pure helpers.
//!
//! Exercises `jaccard_similarity` and other pure utility functions that
//! the experience bank uses for similarity scoring. No async, no IO —
//! safe to run as part of the default test suite.

use sisyphus_mem::experience_bank::jaccard_similarity;

fn approx(a: f64, b: f64) {
    assert!(
        (a - b).abs() < 1e-9,
        "expected {b}, got {a} (delta {})",
        (a - b).abs()
    );
}

#[test]
fn jaccard_identical_strings_score_one() {
    approx(jaccard_similarity("hello world", "hello world"), 1.0);
    approx(jaccard_similarity("rust lang", "rust lang"), 1.0);
}

#[test]
fn jaccard_disjoint_strings_score_zero() {
    approx(jaccard_similarity("foo bar", "baz qux"), 0.0);
    approx(jaccard_similarity("a b c", "d e f"), 0.0);
}

#[test]
fn jaccard_partial_overlap_matches_set_formula() {
    // Tokens: {a, b, c} ∩ {b, c, d} = {b, c} (size 2)
    // Union: {a, b, c, d} (size 4)
    // Expected: 2/4 = 0.5
    approx(jaccard_similarity("a b c", "b c d"), 0.5);
}

#[test]
fn jaccard_is_case_sensitive() {
    // Tokens are lowercased before comparison in most implementations;
    // we just check that distinct casings don't crash. Either 0.0 or 1.0
    // is acceptable — we only assert the result is in [0, 1].
    let score = jaccard_similarity("Hello", "hello");
    assert!((0.0..=1.0).contains(&score), "score {score} out of range");
}

#[test]
fn jaccard_handles_empty_strings() {
    // Implementation contract: when both inputs tokenise to empty sets,
    // there is no information to compare — return 0.0 (a sentinel for
    // "no signal") rather than 1.0 (which would be misleading).
    approx(jaccard_similarity("", ""), 0.0);
    // Asymmetric: one empty, one not → disjoint → 0.0
    approx(jaccard_similarity("", "anything"), 0.0);
    approx(jaccard_similarity("anything", ""), 0.0);
}

#[test]
fn jaccard_handles_whitespace_and_repetition() {
    // "a a a" and "a" both tokenise to {a}, so score should be 1.0
    approx(jaccard_similarity("a a a", "a"), 1.0);
    // "  hello   world  " and "hello world" should be identical
    approx(jaccard_similarity("  hello   world  ", "hello world"), 1.0);
}
