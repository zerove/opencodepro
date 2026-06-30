// ── OpenCode color theme ─────────────────────────────────
// Aligned with opencode's official Dark/Light palette.
// Reference: opencode internal/tui/theme/opencode.go

use ratatui::style::Color;

/// Theme mode: Dark or Light.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Dark,
    Light,
}

impl ThemeMode {
    #[allow(dead_code)]
    pub fn is_dark(self) -> bool {
        matches!(self, ThemeMode::Dark)
    }
}

/// Semantic color set for the TUI.
/// All colors are expressed as `ratatui::style::Color` constants.
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
    pub primary: Color,    // Brand accent
    pub secondary: Color,  // Secondary accent
    pub accent: Color,     // Tertiary accent
    pub error: Color,
    pub warning: Color,
    pub success: Color,
    pub info: Color,
    pub emphasized: Color,
    // Derived helpers
    pub tab_active_bg: Color,
    pub tab_inactive_bg: Color,
    pub status_bg: Color,
    pub status_fg: Color,
    pub scrollbar: Color,
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            mode: ThemeMode::Dark,
            bg:               Color::Rgb(0x21, 0x21, 0x21),
            bg_secondary:     Color::Rgb(0x25, 0x25, 0x25),
            bg_darker:        Color::Rgb(0x12, 0x12, 0x12),
            fg:               Color::Rgb(0xe0, 0xe0, 0xe0),
            muted:            Color::Rgb(0x6a, 0x6a, 0x6a),
            selection:        Color::Rgb(0x30, 0x30, 0x30),
            border:           Color::Rgb(0x4b, 0x4c, 0x5c),
            primary:          Color::Rgb(0xfa, 0xb2, 0x83), // Orange/Gold
            secondary:        Color::Rgb(0x5c, 0x9c, 0xf5), // Blue
            accent:           Color::Rgb(0x9d, 0x7c, 0xd8), // Purple
            error:            Color::Rgb(0xe0, 0x6c, 0x75), // Red
            warning:          Color::Rgb(0xf5, 0xa7, 0x42), // Orange
            success:          Color::Rgb(0x7f, 0xd8, 0x8f), // Green
            info:             Color::Rgb(0x56, 0xb6, 0xc2), // Cyan
            emphasized:       Color::Rgb(0xe5, 0xc0, 0x7b), // Yellow
            tab_active_bg:    Color::Rgb(0x25, 0x25, 0x25),
            tab_inactive_bg:  Color::Rgb(0x1a, 0x1a, 0x1a),
            status_bg:        Color::Rgb(0x12, 0x12, 0x12),
            status_fg:        Color::Rgb(0x6a, 0x6a, 0x6a),
            scrollbar:        Color::Rgb(0x4b, 0x4c, 0x5c),
        }
    }

    pub fn light() -> Self {
        Self {
            mode: ThemeMode::Light,
            bg:               Color::Rgb(0xf8, 0xf8, 0xf8),
            bg_secondary:     Color::Rgb(0xf0, 0xf0, 0xf0),
            bg_darker:        Color::Rgb(0xff, 0xff, 0xff),
            fg:               Color::Rgb(0x2a, 0x2a, 0x2a),
            muted:            Color::Rgb(0x8a, 0x8a, 0x8a),
            selection:        Color::Rgb(0xe5, 0xe5, 0xe6),
            border:           Color::Rgb(0xd3, 0xd3, 0xd3),
            primary:          Color::Rgb(0x3b, 0x7d, 0xd8), // Blue
            secondary:        Color::Rgb(0x7b, 0x5b, 0xb6), // Purple
            accent:           Color::Rgb(0xd6, 0x8c, 0x27), // Gold
            error:            Color::Rgb(0xd1, 0x38, 0x3d), // Red
            warning:          Color::Rgb(0xd6, 0x8c, 0x27), // Orange
            success:          Color::Rgb(0x3d, 0x9a, 0x57), // Green
            info:             Color::Rgb(0x31, 0x87, 0x95), // Cyan
            emphasized:       Color::Rgb(0xb0, 0x85, 0x1f), // Yellow
            tab_active_bg:    Color::Rgb(0xf0, 0xf0, 0xf0),
            tab_inactive_bg:  Color::Rgb(0xe8, 0xe8, 0xe8),
            status_bg:        Color::Rgb(0xff, 0xff, 0xff),
            status_fg:        Color::Rgb(0x8a, 0x8a, 0x8a),
            scrollbar:        Color::Rgb(0xd3, 0xd3, 0xd3),
        }
    }

    /// Get border style for active (focused) panel
    #[allow(dead_code)]
    pub fn active_border(self) -> ratatui::style::Style {
        ratatui::style::Style::default().fg(self.primary)
    }

    /// Get border style for inactive panel
    #[allow(dead_code)]
    pub fn inactive_border(self) -> ratatui::style::Style {
        ratatui::style::Style::default().fg(self.border)
    }
}
