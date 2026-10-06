//! The app's look in one place: palette, egui visuals and spacing, and the section headers every
//! panel uses, so colours and rhythm stay consistent as features are added.

// The one place UI colours are constructed (see clippy.toml).
#![allow(clippy::disallowed_methods)]

use egui::{Color32, CornerRadius, FontId, Stroke, TextStyle};

/// Blue: active modulation, selection, the thing to press next.
pub const ACCENT: Color32 = Color32::from_rgb(120, 200, 255);
/// Translucent accent for filled buttons that want attention (Apply when the editor is dirty).
pub const ACCENT_FILL: Color32 = Color32::from_rgb(34, 72, 102);
/// Amber: MIDI, learn mode, stuck badges, soft warnings.
pub const WARN: Color32 = Color32::from_rgb(255, 190, 90);
/// Red: errors, recording.
pub const ERROR: Color32 = Color32::from_rgb(235, 105, 105);
/// Green: compiled, healthy.
pub const OK: Color32 = Color32::from_rgb(130, 205, 150);

/// Backgrounds, darkest to lightest.
pub const VIEWPORT_BG: Color32 = Color32::from_rgb(9, 10, 12);
pub const EDITOR_BG: Color32 = Color32::from_rgb(15, 16, 19);
pub const CHROME: Color32 = Color32::from_rgb(20, 22, 26);
pub const PANEL: Color32 = Color32::from_rgb(25, 27, 32);
pub const STRIPE: Color32 = Color32::from_rgb(30, 33, 38);
pub const METER_BG: Color32 = Color32::from_rgb(34, 37, 43);

pub const EDITOR_STROKE: Color32 = Color32::from_rgb(42, 46, 54);
pub const VIEWPORT_FRAME: Color32 = Color32::from_rgb(48, 52, 60);
const SEPARATOR: Color32 = Color32::from_rgb(46, 50, 58);
const SECTION_TEXT: Color32 = Color32::from_rgb(165, 171, 184);

/// Installs the theme. Call once with the creation context.
pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();

    style.text_styles = [
        (TextStyle::Small, FontId::proportional(10.5)),
        (TextStyle::Body, FontId::proportional(13.0)),
        (TextStyle::Button, FontId::proportional(13.0)),
        (TextStyle::Heading, FontId::proportional(16.0)),
        (TextStyle::Monospace, FontId::monospace(12.0)),
    ]
    .into();

    let s = &mut style.spacing;
    s.item_spacing = egui::vec2(8.0, 6.0);
    s.button_padding = egui::vec2(8.0, 3.0);
    s.interact_size.y = 20.0;
    s.slider_width = 120.0;
    s.indent = 14.0;

    let v = &mut style.visuals;
    *v = egui::Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.extreme_bg_color = EDITOR_BG;
    v.faint_bg_color = STRIPE;
    v.code_bg_color = METER_BG;
    v.hyperlink_color = ACCENT;
    v.selection.bg_fill = Color32::from_rgb(40, 90, 135);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.slider_trailing_fill = true;
    v.window_corner_radius = CornerRadius::same(8);
    v.menu_corner_radius = CornerRadius::same(6);
    v.window_stroke = Stroke::new(1.0, SEPARATOR);
    v.window_shadow.blur = 24;
    v.window_shadow.offset = [0, 6];

    let w = &mut v.widgets;
    w.noninteractive.bg_stroke = Stroke::new(1.0, SEPARATOR);
    w.noninteractive.fg_stroke = Stroke::new(1.0, Color32::from_gray(150));
    w.inactive.weak_bg_fill = Color32::from_rgb(44, 48, 56);
    w.inactive.bg_fill = Color32::from_rgb(44, 48, 56);
    w.inactive.fg_stroke = Stroke::new(1.0, Color32::from_gray(200));
    w.hovered.weak_bg_fill = Color32::from_rgb(58, 63, 73);
    w.hovered.bg_fill = Color32::from_rgb(58, 63, 73);
    w.hovered.bg_stroke = Stroke::new(1.0, Color32::from_gray(120));
    w.active.weak_bg_fill = Color32::from_rgb(52, 80, 110);
    w.active.bg_fill = Color32::from_rgb(52, 80, 110);
    w.active.bg_stroke = Stroke::new(1.0, ACCENT);
    w.open.weak_bg_fill = Color32::from_rgb(38, 42, 50);
    w.open.bg_fill = PANEL;
    for ws in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        ws.corner_radius = CornerRadius::same(4);
    }

    ctx.set_style_of(egui::Theme::Dark, style);
    ctx.set_theme(egui::ThemePreference::Dark);
}

/// Frame for the top and bottom bars: a shade darker than the side panel so the chrome reads as
/// a separate layer.
pub fn chrome_frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::side_top_panel(style).fill(CHROME)
}

/// A small uppercase section title, without the leading rule (for headers that carry buttons).
pub fn section_label(ui: &mut egui::Ui, title: &str) {
    ui.label(egui::RichText::new(title.to_uppercase()).size(11.0).strong().color(SECTION_TEXT));
}

/// Starts a new section in a side panel: a rule, some air, then the title.
pub fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(8.0);
    ui.separator();
    ui.add_space(4.0);
    section_label(ui, title);
    ui.add_space(2.0);
}
