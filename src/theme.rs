//! Theme preference, resolution, colour tokens, and the persisted settings
//! file. UI-toolkit independent so it can be unit tested.
//!
//! Dark is FontStrike's original look and the default. Light keeps the same
//! orange accent family, darkened until text and controls meet WCAG AA.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// What the user chose in View → Theme.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThemePref {
    #[default]
    Dark,
    Light,
    /// Follow the operating system.
    System,
}

impl ThemePref {
    pub const ALL: [ThemePref; 3] = [ThemePref::Dark, ThemePref::Light, ThemePref::System];

    pub fn label(self) -> &'static str {
        match self {
            ThemePref::Dark => "Dark",
            ThemePref::Light => "Light",
            ThemePref::System => "System",
        }
    }
    fn key(self) -> &'static str {
        match self {
            ThemePref::Dark => "dark",
            ThemePref::Light => "light",
            ThemePref::System => "system",
        }
    }
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "dark" => Some(ThemePref::Dark),
            "light" => Some(ThemePref::Light),
            "system" => Some(ThemePref::System),
            _ => None,
        }
    }
}

/// The theme actually drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Dark,
    Light,
}

/// Resolves a preference. `system` is the OS theme when known; an unknown
/// system theme falls back to FontStrike's default, Dark.
pub fn resolve(pref: ThemePref, system: Option<Mode>) -> Mode {
    match pref {
        ThemePref::Dark => Mode::Dark,
        ThemePref::Light => Mode::Light,
        ThemePref::System => system.unwrap_or(Mode::Dark),
    }
}

pub type Rgb = (u8, u8, u8);

/// Colour tokens for one mode.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    /// Window background behind the start card.
    pub bg: Rgb,
    /// Glyph editing canvas.
    pub canvas: Rgb,
    /// Side panels, toolbar, status bar.
    pub panel: Rgb,
    /// Buttons, inputs, raised surfaces.
    pub raised: Rgb,
    pub hovered: Rgb,
    pub pressed: Rgb,
    pub hover_stroke: Rgb,
    /// Text field background.
    pub field: Rgb,
    pub line: Rgb,
    pub guide: Rgb,
    pub text: Rgb,
    pub muted: Rgb,
    /// Accent for text, outlines, focus and selected controls.
    pub accent: Rgb,
    /// Fill of primary buttons and the active tool.
    pub accent_fill: Rgb,
    /// Text on `accent_fill`.
    pub on_accent: Rgb,
    /// Baseline guide.
    pub accent_dim: Rgb,
    pub select: Rgb,
    pub selection_bg: Rgb,
    pub success: Rgb,
    pub warn: Rgb,
    pub danger: Rgb,
    pub danger_fill: Rgb,
    pub on_danger: Rgb,
    pub component: Rgb,
    /// Advance box behind the glyph.
    pub em_box: Rgb,
    /// Filled preview behind the editable outline.
    pub glyph_fill: Rgb,
    pub tile: Rgb,
    pub tile_selected: Rgb,
    /// Kerned text preview strip.
    pub paper: Rgb,
    pub ink: Rgb,
    /// Kerning value labels and the selected-glyph underline in the strip.
    pub paper_mark: Rgb,
    /// Underline marking the selected glyph in the strip.
    pub paper_underline: Rgb,
    /// Light plate behind the PWDS logo (dark artwork).
    pub badge: Rgb,
}

/// FontStrike's original dark look (unchanged; the default).
pub const DARK: Palette = Palette {
    bg: (17, 20, 24),
    canvas: (21, 25, 30),
    panel: (27, 32, 38),
    raised: (35, 41, 48),
    hovered: (46, 54, 63),
    pressed: (58, 68, 79),
    hover_stroke: (92, 104, 116),
    field: (14, 17, 20),
    line: (52, 60, 69),
    guide: (64, 74, 84),
    text: (230, 235, 236),
    muted: (158, 170, 176),
    accent: (242, 178, 102),
    accent_fill: (242, 178, 102),
    on_accent: (28, 22, 14),
    accent_dim: (140, 100, 58),
    select: (110, 182, 255),
    selection_bg: (56, 86, 120),
    success: (126, 214, 152),
    warn: (246, 204, 104),
    danger: (255, 128, 118),
    danger_fill: (170, 60, 52),
    on_danger: (255, 255, 255),
    component: (132, 160, 214),
    em_box: (25, 30, 36),
    glyph_fill: (66, 78, 90),
    tile: (31, 37, 43),
    tile_selected: (54, 45, 34),
    paper: (238, 234, 226),
    ink: (24, 22, 20),
    paper_mark: (150, 82, 20),
    paper_underline: (196, 120, 40),
    badge: (240, 238, 232),
};

/// Light variant: warm neutrals, the same orange family darkened for AA.
pub const LIGHT: Palette = Palette {
    bg: (240, 239, 235),
    canvas: (255, 255, 255),
    panel: (247, 247, 245),
    raised: (233, 232, 228),
    hovered: (223, 222, 217),
    pressed: (210, 209, 203),
    hover_stroke: (150, 148, 140),
    field: (255, 255, 255),
    line: (214, 213, 207),
    guide: (196, 195, 188),
    text: (28, 28, 26),
    muted: (88, 88, 82),
    accent: (166, 82, 6),
    accent_fill: (166, 82, 6),
    on_accent: (255, 255, 255),
    accent_dim: (201, 120, 44),
    select: (22, 104, 200),
    selection_bg: (196, 220, 248),
    success: (28, 116, 58),
    warn: (128, 84, 0),
    danger: (176, 36, 28),
    danger_fill: (176, 36, 28),
    on_danger: (255, 255, 255),
    component: (52, 92, 170),
    em_box: (248, 245, 239),
    glyph_fill: (214, 218, 225),
    tile: (255, 255, 255),
    tile_selected: (252, 234, 210),
    paper: (255, 254, 250),
    ink: (28, 28, 26),
    paper_mark: (150, 70, 8),
    paper_underline: (201, 120, 44),
    badge: (240, 238, 232),
};

pub fn palette(mode: Mode) -> &'static Palette {
    match mode {
        Mode::Dark => &DARK,
        Mode::Light => &LIGHT,
    }
}

fn channel(c: u8) -> f64 {
    let c = c as f64 / 255.0;
    if c <= 0.03928 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// WCAG 2.x relative luminance.
pub fn luminance(c: Rgb) -> f64 {
    0.2126 * channel(c.0) + 0.7152 * channel(c.1) + 0.0722 * channel(c.2)
}

/// WCAG 2.x contrast ratio (1..21).
pub fn contrast(a: Rgb, b: Rgb) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Persisted user settings (`key = value` lines; unknown lines are kept).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    pub theme: ThemePref,
    other: Vec<String>,
}

impl Settings {
    /// Reads settings. A missing, unreadable, or malformed file yields the
    /// defaults instead of an error, so a bad file never blocks startup.
    pub fn load(path: &Path) -> Settings {
        let Ok(text) = fs::read_to_string(path) else {
            return Settings::default();
        };
        let mut s = Settings::default();
        for line in text.lines() {
            match line.split_once('=') {
                Some((k, v)) if k.trim().eq_ignore_ascii_case("theme") => {
                    if let Some(t) = ThemePref::parse(v) {
                        s.theme = t;
                    }
                }
                _ if line.trim().is_empty() => {}
                _ => s.other.push(line.to_string()),
            }
        }
        s
    }

    /// Writes settings atomically (temporary file, then rename).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent()
            && !dir.as_os_str().is_empty()
        {
            fs::create_dir_all(dir)?;
        }
        let mut text = String::from("# FontStrike settings\n");
        text.push_str(&format!("theme = {}\n", self.theme.key()));
        for line in self
            .other
            .iter()
            .filter(|l| !l.starts_with("# FontStrike settings"))
        {
            text.push_str(line);
            text.push('\n');
        }
        let tmp = path.with_extension("tmp");
        {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(text.as_bytes())?;
            f.sync_all()?;
        }
        fs::rename(&tmp, path)
    }

    /// `%APPDATA%\FontStrike\settings.ini` on Windows, the XDG config
    /// directory elsewhere.
    pub fn default_path() -> Option<PathBuf> {
        let base = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(base.join("FontStrike").join("settings.ini"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolution_follows_preference_and_system() {
        assert_eq!(ThemePref::default(), ThemePref::Dark);
        for system in [None, Some(Mode::Light), Some(Mode::Dark)] {
            assert_eq!(resolve(ThemePref::Dark, system), Mode::Dark);
            assert_eq!(resolve(ThemePref::Light, system), Mode::Light);
        }
        assert_eq!(resolve(ThemePref::System, Some(Mode::Light)), Mode::Light);
        assert_eq!(resolve(ThemePref::System, Some(Mode::Dark)), Mode::Dark);
        assert_eq!(
            resolve(ThemePref::System, None),
            Mode::Dark,
            "unknown OS theme falls back to Dark"
        );
        for p in ThemePref::ALL {
            assert_eq!(ThemePref::parse(p.key()), Some(p));
            assert_eq!(ThemePref::parse(&p.label().to_uppercase()), Some(p));
        }
        assert_eq!(ThemePref::parse("sepia"), None);
    }

    #[test]
    fn contrast_formula_matches_wcag_reference_values() {
        assert!((contrast((0, 0, 0), (255, 255, 255)) - 21.0).abs() < 1e-9);
        assert!((contrast((119, 119, 119), (255, 255, 255)) - 4.48).abs() < 0.01);
    }

    #[test]
    fn both_palettes_meet_wcag_aa() {
        for (name, p) in [("dark", DARK), ("light", LIGHT)] {
            let text_pairs = [
                ("text/panel", p.text, p.panel),
                ("text/canvas", p.text, p.canvas),
                ("text/raised", p.text, p.raised),
                ("text/field", p.text, p.field),
                ("muted/panel", p.muted, p.panel),
                ("muted/canvas", p.muted, p.canvas),
                ("accent/panel", p.accent, p.panel),
                ("accent/canvas", p.accent, p.canvas),
                ("on_accent/accent_fill", p.on_accent, p.accent_fill),
                ("on_danger/danger_fill", p.on_danger, p.danger_fill),
                ("success/panel", p.success, p.panel),
                ("warn/panel", p.warn, p.panel),
                ("danger/panel", p.danger, p.panel),
                ("ink/paper", p.ink, p.paper),
                ("paper_mark/paper", p.paper_mark, p.paper),
                ("text/tile_selected", p.text, p.tile_selected),
            ];
            for (what, a, b) in text_pairs {
                let c = contrast(a, b);
                assert!(c >= 4.5, "{name} {what}: {c:.2} < 4.5");
            }
            let ui_pairs = [
                ("accent outline/canvas", p.accent, p.canvas),
                ("focus ring/panel", p.accent, p.panel),
                ("select/canvas", p.select, p.canvas),
                ("component/canvas", p.component, p.canvas),
                ("glyph outline/fill", p.accent, p.glyph_fill),
                ("baseline/canvas", p.accent_dim, p.canvas),
            ];
            for (what, a, b) in ui_pairs {
                let c = contrast(a, b);
                assert!(c >= 3.0, "{name} {what}: {c:.2} < 3.0");
            }
        }
    }

    #[test]
    fn dark_palette_is_the_original_look() {
        assert_eq!(DARK.accent, (242, 178, 102));
        assert_eq!(DARK.panel, (27, 32, 38));
        assert_eq!(DARK.canvas, (21, 25, 30));
    }
}
