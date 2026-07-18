// ── OpenCode-inspired theme ───────────────────────────────
// Reference: opencode's internal/tui/theme/opencode.go
// Uses muted, clean colors with minimal visual noise.

use ratatui::style::Color;

/// Theme mode: Dark or Light.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ThemeMode {
    Dark,
    Light,
}

/// Semantic color set for the TUI.
/// Some fields reserved for future rendering features (scrollbar, status line, etc.).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Theme {
    pub mode: ThemeMode,
    // Surface colors
    pub bg: Color,
    pub bg_secondary: Color,
    pub bg_darker: Color,
    pub fg: Color,
    pub muted: Color,
    pub selection: Color,
    pub border: Color,
    // Semantic colors
    pub primary: Color,   // Brand accent
    pub secondary: Color, // Secondary accent
    pub accent: Color,    // Tertiary accent
    pub error: Color,
    pub warning: Color,
    pub success: Color,
    pub info: Color,
    pub emphasized: Color,
    // Derived helpers
    pub status_bg: Color,
    pub status_fg: Color,
    pub scrollbar: Color,
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            mode: ThemeMode::Dark,
            bg: Color::Rgb(0x1a, 0x1a, 0x1a),
            bg_secondary: Color::Rgb(0x22, 0x22, 0x22),
            bg_darker: Color::Rgb(0x11, 0x11, 0x11),
            fg: Color::Rgb(0xd4, 0xd4, 0xd4),
            muted: Color::Rgb(0x66, 0x66, 0x66),
            selection: Color::Rgb(0x2a, 0x2a, 0x2a),
            border: Color::Rgb(0x33, 0x33, 0x33),
            primary: Color::Rgb(0xf0, 0xa0, 0x60),    // Orange
            secondary: Color::Rgb(0x60, 0xa0, 0xf0),  // Blue
            accent: Color::Rgb(0xa0, 0x80, 0xd0),     // Purple
            error: Color::Rgb(0xd4, 0x60, 0x60),      // Red
            warning: Color::Rgb(0xe0, 0x90, 0x40),    // Orange
            success: Color::Rgb(0x70, 0xc0, 0x80),    // Green
            info: Color::Rgb(0x50, 0xb0, 0xb8),       // Cyan
            emphasized: Color::Rgb(0xd0, 0xb0, 0x60), // Yellow
            status_bg: Color::Rgb(0x11, 0x11, 0x11),
            status_fg: Color::Rgb(0x66, 0x66, 0x66),
            scrollbar: Color::Rgb(0x33, 0x33, 0x33),
        }
    }

    pub fn light() -> Self {
        Self {
            mode: ThemeMode::Light,
            bg: Color::Rgb(0xf5, 0xf5, 0xf5),
            bg_secondary: Color::Rgb(0xee, 0xee, 0xee),
            bg_darker: Color::Rgb(0xff, 0xff, 0xff),
            fg: Color::Rgb(0x33, 0x33, 0x33),
            muted: Color::Rgb(0x88, 0x88, 0x88),
            selection: Color::Rgb(0xe0, 0xe0, 0xe0),
            border: Color::Rgb(0xcc, 0xcc, 0xcc),
            primary: Color::Rgb(0x40, 0x80, 0xd0),    // Blue
            secondary: Color::Rgb(0x70, 0x50, 0xb0),  // Purple
            accent: Color::Rgb(0xc0, 0x80, 0x30),     // Gold
            error: Color::Rgb(0xc0, 0x40, 0x40),      // Red
            warning: Color::Rgb(0xc0, 0x80, 0x30),    // Orange
            success: Color::Rgb(0x40, 0x90, 0x50),    // Green
            info: Color::Rgb(0x30, 0x80, 0x88),       // Cyan
            emphasized: Color::Rgb(0xa0, 0x80, 0x20), // Yellow
            status_bg: Color::Rgb(0xff, 0xff, 0xff),
            status_fg: Color::Rgb(0x88, 0x88, 0x88),
            scrollbar: Color::Rgb(0xcc, 0xcc, 0xcc),
        }
    }
}
