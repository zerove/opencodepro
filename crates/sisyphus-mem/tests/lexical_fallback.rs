//! 集成测试：mem experience_bank（jaccard + LexicalFallbackReason）
//!
//! 覆盖 `sisyphus_mem::experience_bank` 中的纯同步模块：
//! - `jaccard_similarity` 多种输入（相同/包含/空/标点）
//! - `LexicalFallbackReason` Display + Copy + Eq

use sisyphus_mem::experience_bank::{jaccard_similarity, LexicalFallbackReason};

// ---------------------------------------------------------------------------
// jaccard_similarity
// ---------------------------------------------------------------------------

#[test]
fn jaccard_identical() {
    let s = jaccard_similarity("hello world", "hello world");
    assert!((s - 1.0).abs() < 1e-9);
}

#[test]
fn jaccard_disjoint() {
    let s = jaccard_similarity("foo bar", "baz qux");
    assert!(s.abs() < 1e-9);
}

#[test]
fn jaccard_partial_overlap() {
    // "the quick brown fox" vs "the lazy brown dog"
    // tokens: {the, quick, brown, fox} ∩ {the, lazy, brown, dog} = {the, brown} = 2
    // union: 4 + 4 - 2 = 6
    // score = 2/6 = 0.333
    let s = jaccard_similarity("the quick brown fox", "the lazy brown dog");
    assert!((s - 2.0 / 6.0).abs() < 1e-9);
}

#[test]
fn jaccard_case_insensitive() {
    let s1 = jaccard_similarity("Hello World", "hello world");
    assert!((s1 - 1.0).abs() < 1e-9);
    let s2 = jaccard_similarity("HELLO", "hello");
    assert!((s2 - 1.0).abs() < 1e-9);
}

#[test]
fn jaccard_substring_is_partial() {
    // "hello" tokens = {hello}, "hello world" tokens = {hello, world}
    // inter=1, union=2 → 0.5
    let s = jaccard_similarity("hello", "hello world");
    assert!((s - 0.5).abs() < 1e-9);
}

#[test]
fn jaccard_punctuation_separates() {
    // "foo,bar;baz" splits into {foo, bar, baz}
    let s = jaccard_similarity("foo,bar;baz", "foo bar baz");
    assert!((s - 1.0).abs() < 1e-9);
}

#[test]
fn jaccard_empty_both_zero() {
    // Both empty → defined as 0.0 (no info)
    let s = jaccard_similarity("", "");
    assert_eq!(s, 0.0);
}

#[test]
fn jaccard_one_empty() {
    // {a} ∩ {} = 0; union = 1
    let s = jaccard_similarity("a", "");
    assert_eq!(s, 0.0);
}

#[test]
fn jaccard_whitespace_only() {
    // "   " → empty after filter
    let s = jaccard_similarity("   ", "   ");
    assert_eq!(s, 0.0);
}

#[test]
fn jaccard_numbers_and_words() {
    // "abc 123" tokens: {abc, 123}; "abc 456" tokens: {abc, 456}
    // intersection = {abc} = 1; union = 3; score = 1/3
    let s = jaccard_similarity("abc 123", "abc 456");
    assert!((s - 1.0 / 3.0).abs() < 1e-9);
}

#[test]
fn jaccard_unicode_alphanumeric() {
    // Chinese chars are alphanumeric per Rust
    let s = jaccard_similarity("你好 世界", "你好");
    // {你好, 世界} ∩ {你好} = 1; union = 2; 1/2
    assert!((s - 0.5).abs() < 1e-9);
}

#[test]
fn jaccard_duplicate_tokens_counted_once() {
    // "foo foo bar" tokens as set = {foo, bar}
    // "foo bar" tokens as set = {foo, bar}
    let s = jaccard_similarity("foo foo bar", "foo bar");
    assert!((s - 1.0).abs() < 1e-9);
}

// ---------------------------------------------------------------------------
// LexicalFallbackReason
// ---------------------------------------------------------------------------

#[test]
fn fallback_reason_display_embed_failed() {
    assert_eq!(format!("{}", LexicalFallbackReason::EmbedFailed), "embedding call failed");
}

#[test]
fn fallback_reason_copy_eq() {
    let a = LexicalFallbackReason::EmbedFailed;
    let b = a; // Copy
    assert_eq!(a, b);
}

#[test]
fn fallback_reason_eq_single_variant() {
    // Only one variant; just verify Eq holds.
    let x = LexicalFallbackReason::EmbedFailed;
    let y = LexicalFallbackReason::EmbedFailed;
    assert_eq!(x, y);
}

#[test]
fn fallback_reason_clone() {
    let x = LexicalFallbackReason::EmbedFailed;
    let y = x.clone();
    assert_eq!(x, y);
}

#[test]
fn fallback_reason_debug() {
    let x = LexicalFallbackReason::EmbedFailed;
    let s = format!("{x:?}");
    assert!(s.contains("EmbedFailed"));
}
