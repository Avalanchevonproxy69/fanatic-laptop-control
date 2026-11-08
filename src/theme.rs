//! Palette, fonts and egui style. Mirrors `docs/design/mockup.html`.
//!
//! Neutral greys for the chrome with one orange accent; the ironbow thermal ramp is reserved
//! for data (the chassis heat map and the fan curve fill) and never used as decoration.

use std::{path::PathBuf, sync::Arc};

use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, FontId, Stroke};

/// Title bar, sidebar, status bar and plot backgrounds.
pub const CHROME: Color32 = Color32::from_rgb(0x1E, 0x1F, 0x22);
/// Content area.
pub const BASE: Color32 = Color32::from_rgb(0x26, 0x27, 0x2B);
/// Group boxes and list surfaces.
pub const SURFACE: Color32 = Color32::from_rgb(0x2E, 0x30, 0x35);
/// Selected nav row and selected segment.
pub const SELECTED: Color32 = Color32::from_rgb(0x40, 0x42, 0x49);
pub const LINE: Color32 = Color32::from_rgb(0x3A, 0x3D, 0x43);
pub const TEXT: Color32 = Color32::from_rgb(0xE3, 0xE5, 0xE8);
pub const MUTED: Color32 = Color32::from_rgb(0xA4, 0xA7, 0xAE);
pub const FAINT: Color32 = Color32::from_rgb(0x7D, 0x80, 0x88);
/// The one accent: primary buttons, toggles, the selected nav pill, the dragged curve point.
pub const ACCENT: Color32 = Color32::from_rgb(0xF0, 0x8C, 0x35);

/// Ironbow thermal ramp: black, indigo, purple, magenta, red-orange, orange, yellow, white.
pub const IRONBOW: [Color32; 8] =
    [rgb(0x050314), rgb(0x2A0B5E), rgb(0x6A0F8E), rgb(0xB5207F), rgb(0xE8483F), rgb(0xF98A16), rgb(0xFCC93A), rgb(0xFFF6D0)];

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// Coldest and hottest temperatures on the shared thermal scale, in degrees Celsius.
pub const SCALE_MIN_C: f32 = 25.0;
pub const SCALE_MAX_C: f32 = 100.0;

/// Sample the ironbow ramp at `t` in `0..=1`.
pub fn ironbow(t: f32) -> Color32 {
    let x = t.clamp(0.0, 1.0) * (IRONBOW.len() - 1) as f32;
    let i = (x.floor() as usize).min(IRONBOW.len() - 2);
    let (a, b, f) = (IRONBOW[i], IRONBOW[i + 1], x - i as f32);
    let mix = |p: u8, q: u8| (p as f32 + (q as f32 - p as f32) * f).round() as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

/// Color of a temperature on the shared scale.
pub fn temp_color(celsius: f32) -> Color32 {
    ironbow((celsius - SCALE_MIN_C) / (SCALE_MAX_C - SCALE_MIN_C))
}

const DISPLAY: &str = "display";

/// Big numerals (RPM, percentages): SF Pro Display when installed.
pub fn display(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(DISPLAY.into()))
}

pub fn text(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

/// Load the first of `candidates` found in the system or per-user font folder.
fn load_first(defs: &mut FontDefinitions, key: &str, candidates: &[&str]) -> bool {
    let mut dirs = vec![PathBuf::from(r"C:\Windows\Fonts")];
    dirs.extend(std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join(r"Microsoft\Windows\Fonts")));
    let found = dirs.iter().flat_map(|d| candidates.iter().map(move |n| d.join(n))).find_map(|p| std::fs::read(p).ok());
    found.map(|bytes| defs.font_data.insert(key.to_owned(), Arc::new(FontData::from_owned(bytes)))).is_some()
}

/// SF Pro / SF Mono if installed, otherwise Segoe UI Variable, otherwise egui's bundled
/// fonts. Font files are only read from disk, never embedded.
fn install_fonts(ctx: &egui::Context) {
    let mut defs = FontDefinitions::default();
    let mut proportional = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    if load_first(&mut defs, "ui-text", &["SF-Pro-Text-Regular.otf", "SF-Pro.ttf", "SegUIVar.ttf", "segoeui.ttf"]) {
        proportional.insert(0, "ui-text".to_owned());
    }
    let mut display_family = proportional.clone();
    if load_first(
        &mut defs,
        "ui-display",
        &["SF-Pro-Display-Medium.otf", "SF-Pro-Display-Regular.otf", "SF-Pro.ttf", "SegUIVar.ttf"],
    ) {
        display_family.insert(0, "ui-display".to_owned());
    }
    if load_first(&mut defs, "ui-mono", &["SF-Mono-Regular.otf", "CascadiaMono.ttf", "consola.ttf"]) {
        defs.families.entry(FontFamily::Monospace).or_default().insert(0, "ui-mono".to_owned());
    }
    defs.families.insert(FontFamily::Proportional, proportional);
    defs.families.insert(FontFamily::Name(DISPLAY.into()), display_family);
    ctx.set_fonts(defs);
}

/// Apply fonts and the neutral dark visuals (always dark: thermal colors need a dark ground).
pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);
    let mut v = egui::Visuals::dark();
    (v.panel_fill, v.window_fill, v.extreme_bg_color, v.faint_bg_color) = (BASE, SURFACE, CHROME, SURFACE);
    v.override_text_color = Some(TEXT);
    v.selection.bg_fill = ACCENT.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.window_stroke = Stroke::new(1.0, LINE);
    let (button, hover) = (Color32::from_rgb(0x35, 0x37, 0x3C), Color32::from_rgb(0x3A, 0x3C, 0x42));
    let w = &mut v.widgets;
    for (wv, fill) in [
        (&mut w.noninteractive, SURFACE),
        (&mut w.inactive, button),
        (&mut w.hovered, hover),
        (&mut w.active, SELECTED),
        (&mut w.open, SELECTED),
    ] {
        (wv.bg_fill, wv.weak_bg_fill) = (fill, fill);
        wv.corner_radius = egui::CornerRadius::same(5);
        wv.bg_stroke = Stroke::new(1.0, LINE);
    }
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.set_visuals_of(egui::Theme::Dark, v);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(10.0, 8.0);
        s.spacing.button_padding = egui::vec2(12.0, 6.0);
    });
}
