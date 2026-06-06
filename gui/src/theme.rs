//! Curated themes. The web shipped 14 CodeMirror themes; egui has no CodeMirror, so we
//! pick a curated handful and rebuild each as an egui `Visuals` (app chrome) paired with a
//! syntect theme (the code colors in the diff). The chrome background is taken from the
//! syntect theme so the window matches the code area. UI labels are English.

use egui::Color32;
use two_face::theme::EmbeddedThemeName;

pub struct ThemeDef {
    pub id: &'static str,
    pub label: &'static str,
    pub dark: bool,
    /// The syntect theme used to color code in the diff.
    pub syntect: EmbeddedThemeName,
}

/// The curated set, in menu order. Maps to the syntect themes bundled by `two-face`
/// (Tokyo Night isn't bundled, so the dark set leans on Dracula/Nord/Gruvbox/VS Code).
pub const THEMES: &[ThemeDef] = &[
    ThemeDef {
        id: "githubDark",
        label: "GitHub Dark",
        dark: true,
        // two-face's `Github` is the *light* GitHub theme (white bg); Coldark-Dark is the closest
        // bundled dark substitute (#111b27 ≈ GitHub's #0d1117).
        syntect: EmbeddedThemeName::ColdarkDark,
    },
    ThemeDef {
        id: "dracula",
        label: "Dracula",
        dark: true,
        syntect: EmbeddedThemeName::Dracula,
    },
    ThemeDef {
        id: "nord",
        label: "Nord",
        dark: true,
        syntect: EmbeddedThemeName::Nord,
    },
    ThemeDef {
        id: "gruvboxDark",
        label: "Gruvbox Dark",
        dark: true,
        syntect: EmbeddedThemeName::GruvboxDark,
    },
    ThemeDef {
        id: "monokai",
        label: "Monokai",
        dark: true,
        syntect: EmbeddedThemeName::MonokaiExtended,
    },
    ThemeDef {
        id: "githubLight",
        label: "GitHub Light",
        dark: false,
        syntect: EmbeddedThemeName::InspiredGithub,
    },
    ThemeDef {
        id: "solarizedLight",
        label: "Solarized Light",
        dark: false,
        syntect: EmbeddedThemeName::SolarizedLight,
    },
];

pub const DEFAULT: &str = "githubDark";

/// Look up a theme by id, falling back to the default.
pub fn def(id: &str) -> &'static ThemeDef {
    THEMES
        .iter()
        .find(|t| t.id == id)
        .unwrap_or_else(|| def_default())
}

fn def_default() -> &'static ThemeDef {
    THEMES
        .iter()
        .find(|t| t.id == DEFAULT)
        .unwrap_or(&THEMES[0])
}

/// Apply a theme's egui `Visuals` to the context (the code colors are set separately on the
/// `Highlighter`). The chrome background is pulled from the syntect theme so they match.
pub fn apply(ctx: &egui::Context, id: &str) {
    let d = def(id);
    let mut v = if d.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    if let Some(bg) = two_face::theme::extra().get(d.syntect).settings.background {
        let c = Color32::from_rgb(bg.r, bg.g, bg.b);
        v.panel_fill = c;
        v.window_fill = c;
        // A slightly darker extreme bg for scroll troughs / text edits keeps depth.
        v.extreme_bg_color = darken(c, 0.85);
    }
    ctx.set_visuals(v);
}

fn darken(c: Color32, f: f32) -> Color32 {
    Color32::from_rgb(
        (c.r() as f32 * f) as u8,
        (c.g() as f32 * f) as u8,
        (c.b() as f32 * f) as u8,
    )
}
