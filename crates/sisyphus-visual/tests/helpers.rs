//! Integration tests for sisyphus-visual pure helpers.
//!
//! Exercises the sync, in-process helpers (`is_png`, `base64_encode`,
//! `simulated_visual_state`) that don't require an LLM. Async node
//! processing is exercised separately by the unit tests under `src/`.

use sisyphus_visual::{base64_encode, is_png, simulated_visual_state};

#[test]
fn is_png_recognises_png_magic() {
    // PNG signature: 0x89 0x50 0x4E 0x47 0x0D 0x0A 0x1A 0x0A
    let png_sig: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    assert!(
        is_png(&png_sig),
        "8-byte PNG signature should be recognised"
    );

    // Add some trailing payload to make it look like a real PNG stream
    let mut png = png_sig.to_vec();
    png.extend_from_slice(&[0x00, 0x00, 0x00, 0x0D]);
    assert!(is_png(&png), "full PNG frame should be recognised");
}

#[test]
fn is_png_rejects_non_png() {
    assert!(!is_png(b"not a png"));
    assert!(!is_png(&[0xFF, 0xD8, 0xFF, 0xE0])); // JPEG magic
    assert!(!is_png(b""), "empty buffer is not a PNG");
    assert!(
        !is_png(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A]),
        "truncated PNG (7 bytes) is not a PNG"
    );
}

#[test]
fn base64_encode_handles_empty_and_ascii() {
    assert_eq!(base64_encode(b""), "");
    assert_eq!(base64_encode(b"f"), "Zg==");
    assert_eq!(base64_encode(b"fo"), "Zm8=");
    assert_eq!(base64_encode(b"foo"), "Zm9v");
    assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
    assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
    assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
}

#[test]
fn base64_encode_handles_binary_payload() {
    let bytes: [u8; 3] = [0x00, 0xFF, 0x80];
    // Standard RFC 4648 base64 of [0x00, 0xFF, 0x80] = "AP+A"
    let out = base64_encode(&bytes);
    assert_eq!(out, "AP+A");
}

#[test]
fn simulated_visual_state_is_deterministic_per_seed() {
    let a1 = simulated_visual_state("seed-1");
    let a2 = simulated_visual_state("seed-1");
    assert_eq!(a1, a2, "same seed must produce identical bytes");

    let b = simulated_visual_state("seed-2");
    assert_ne!(a1, b, "different seeds should differ");
}

#[test]
fn simulated_visual_state_returns_non_empty_for_non_empty_seed() {
    let bytes = simulated_visual_state("any-seed");
    assert!(!bytes.is_empty(), "non-empty seed should produce a payload");
}
