//! The small controls the pages are built from.

use crate::icons;
use crate::theme::{self, ACCENT, MUTED, ROW_HOVER, ROW_SELECTED, TEXT, WARN};
use egui::{
    Align, Color32, CornerRadius, CursorIcon, Frame, Layout, Margin, Pos2, Rect, Response,
    RichText, Sense, Shape, Stroke, StrokeKind, TextureHandle, Ui,
};
use std::collections::HashMap;

/// An on/off switch. Animated, and it reads as on or off at a glance where a checkbox does not.
pub fn switch(ui: &mut Ui, on: &mut bool) -> Response {
    let size = egui::vec2(38.0, 22.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let response = response.on_hover_cursor(CursorIcon::PointingHand);
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_responsive(response.id, *on);
        let track = Color32::from_rgb(70, 60, 70).lerp_to_gamma(ACCENT, t);
        let painter = ui.painter();
        painter.rect(
            rect,
            CornerRadius::same(11),
            track,
            Stroke::new(
                1.0_f32,
                Color32::from_white_alpha(if response.hovered() { 60 } else { 30 }),
            ),
            StrokeKind::Inside,
        );
        let x = egui::lerp((rect.left() + 11.0)..=(rect.right() - 11.0), t);
        painter.circle_filled(egui::pos2(x, rect.center().y), 8.0, Color32::WHITE);
    }
    response
}

/// A switch with a label beside it, as one row.
pub fn switch_row(ui: &mut Ui, on: &mut bool, title: &str, detail: &str) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        changed = switch(ui, on).changed();
        ui.add_space(4.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(RichText::new(title).color(TEXT));
            if !detail.is_empty() {
                ui.add(egui::Label::new(theme::muted(detail).small()).wrap());
            }
        });
    });
    changed
}

/// One choice out of a few, drawn as joined buttons. Returns whether the choice changed.
pub fn segmented<T: Copy + PartialEq>(
    ui: &mut Ui,
    value: &mut T,
    options: &[(T, &str, &str)],
) -> bool {
    let mut changed = false;
    Frame::new()
        .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 12))
        .stroke(Stroke::new(1.0_f32, Color32::from_white_alpha(26)))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for &(option, label, tip) in options {
                    let selected = *value == option;
                    let text = if selected {
                        RichText::new(label).color(Color32::WHITE)
                    } else {
                        RichText::new(label).color(MUTED)
                    };
                    let button = egui::Button::new(text)
                        .fill(if selected {
                            ACCENT
                        } else {
                            Color32::TRANSPARENT
                        })
                        .stroke(Stroke::NONE)
                        .corner_radius(CornerRadius::same(8))
                        .min_size(egui::vec2(72.0, 28.0));
                    let mut response = ui.add(button);
                    if !tip.is_empty() {
                        response = response.on_hover_text(tip);
                    }
                    if response.clicked() && !selected {
                        *value = option;
                        changed = true;
                    }
                }
            });
        });
    changed
}

/// A small upper-case heading over a group of rows.
pub fn section_label(ui: &mut Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(
        RichText::new(text.to_uppercase())
            .size(11.0)
            .strong()
            .color(MUTED.gamma_multiply(0.85)),
    );
    ui.add_space(2.0);
}

/// The page title and its one-line explanation, with room on the right for controls.
pub fn page_header(ui: &mut Ui, title: &str, subtitle: &str, right: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(RichText::new(title).size(22.0).strong().color(TEXT));
            ui.label(theme::muted(subtitle));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), right);
    });
    ui.add_space(10.0);
}

/// A label on the left and a slider filling the rest of the row, its number kept inside.
pub fn slider_row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> Response) -> bool {
    let budget = ui.available_width();
    let mut changed = false;
    ui.allocate_ui_with_layout(
        egui::vec2(budget, 0.0),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_width(budget);
            let label_w = 104.0_f32.min(budget * 0.35);
            ui.allocate_ui_with_layout(
                egui::vec2(label_w, ui.spacing().interact_size.y),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.set_width(label_w);
                    ui.label(theme::muted(label));
                },
            );
            ui.spacing_mut().slider_width = slider_rail_width(ui);
            changed = add(ui).changed();
        },
    );
    changed
}

/// Width of the number button beside a slider, measured for the widest value.
///
/// `slider_width` is only the rail. The value button is drawn after it, so a rail
/// that uses the whole remaining row paints the number outside the window.
fn slider_value_width(ui: &Ui) -> f32 {
    let padded = text_width(ui, "100%", egui::TextStyle::Button.resolve(ui.style()).size)
        + ui.spacing().button_padding.x * 2.0;
    padded.max(ui.spacing().interact_size.x) + 4.0
}

fn slider_rail_width(ui: &Ui) -> f32 {
    let gap = ui.spacing().item_spacing.x;
    (ui.available_width() - gap - slider_value_width(ui)).max(0.0)
}

pub fn text_width(ui: &Ui, text: &str, size: f32) -> f32 {
    ui.painter()
        .layout_no_wrap(
            text.to_owned(),
            egui::FontId::proportional(size),
            Color32::WHITE,
        )
        .size()
        .x
}

/// A label and a one-line text field that fills the row. True once you leave the field.
pub fn text_row(ui: &mut Ui, label: &str, text: &mut String, hint: &str) -> bool {
    let mut done = false;
    let row_w = ui.available_width();
    ui.horizontal(|ui| {
        ui.set_width(row_w);
        let label_w = 104.0_f32.min(row_w * 0.35);
        ui.allocate_ui_with_layout(
            egui::vec2(label_w, ui.spacing().interact_size.y),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.set_width(label_w);
                ui.label(theme::muted(label));
            },
        );
        let field = ui.add(
            egui::TextEdit::singleline(text)
                .hint_text(hint)
                .desired_width(ui.available_width()),
        );
        done = field.lost_focus();
    });
    done
}

/// "HH:MM", in the warning color while it does not read as a time. True on a valid edit.
pub fn clock_field(ui: &mut Ui, id: &str, text: &mut String) -> bool {
    let valid = crate::rules::parse_clock(text).is_some();
    let mut edit = egui::TextEdit::singleline(text)
        .id(egui::Id::new(id))
        .desired_width(52.0)
        .hint_text("HH:MM");
    if !valid {
        edit = edit.text_color(WARN);
    }
    ui.add(edit).changed() && crate::rules::parse_clock(text).is_some()
}

/// The sidebar's pages, each with a small drawn icon so nothing depends on an icon font.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NavIcon {
    Apps,
    Presets,
    Shortcuts,
    AutoPause,
    Settings,
}

pub fn nav_item(ui: &mut Ui, icon: NavIcon, label: &str, selected: bool) -> Response {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 38.0), Sense::click());
    let response = response.on_hover_cursor(CursorIcon::PointingHand);
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let fill = if selected {
            ROW_SELECTED
        } else if response.hovered() {
            ROW_HOVER
        } else {
            Color32::TRANSPARENT
        };
        painter.rect_filled(rect, CornerRadius::same(10), fill);
        if selected {
            let bar = Rect::from_min_size(
                rect.left_top() + egui::vec2(0.0, 10.0),
                egui::vec2(3.0, rect.height() - 20.0),
            );
            painter.rect_filled(bar, CornerRadius::same(2), ACCENT);
        }
        let color = if selected { TEXT } else { MUTED };
        let icon_rect = Rect::from_center_size(
            egui::pos2(rect.left() + 24.0, rect.center().y),
            egui::vec2(16.0, 16.0),
        );
        draw_nav_icon(
            painter,
            icon_rect,
            icon,
            if selected { ACCENT } else { color },
        );
        painter.text(
            egui::pos2(rect.left() + 44.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(15.0),
            color,
        );
    }
    response
}

fn draw_nav_icon(painter: &egui::Painter, r: Rect, icon: NavIcon, color: Color32) {
    let stroke = Stroke::new(1.6_f32, color);
    let c = r.center();
    match icon {
        NavIcon::Apps => {
            let s = r.width() / 2.0 - 1.5;
            for (dx, dy) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
                let min = r.min + egui::vec2(dx * (s + 3.0), dy * (s + 3.0));
                painter.rect_stroke(
                    Rect::from_min_size(min, egui::vec2(s, s)),
                    CornerRadius::same(2),
                    stroke,
                    StrokeKind::Inside,
                );
            }
        }
        NavIcon::Presets => {
            painter.circle_stroke(c, r.width() / 2.0, stroke);
            for (i, angle) in [-2.2_f32, -1.1, 0.0].into_iter().enumerate() {
                let p = c + egui::vec2(angle.cos(), angle.sin()) * (r.width() * 0.25);
                painter.circle_filled(p, 1.9 + i as f32 * 0.1, color);
            }
        }
        NavIcon::Shortcuts => {
            let body = r.shrink2(egui::vec2(0.0, 3.0));
            painter.rect_stroke(body, CornerRadius::same(3), stroke, StrokeKind::Inside);
            for i in 0..3 {
                let x = body.left() + 4.0 + i as f32 * 4.0;
                painter.circle_filled(egui::pos2(x, body.top() + 4.0), 1.0, color);
            }
            painter.line_segment(
                [
                    egui::pos2(body.left() + 4.0, body.bottom() - 3.5),
                    egui::pos2(body.right() - 4.0, body.bottom() - 3.5),
                ],
                stroke,
            );
        }
        NavIcon::AutoPause => {
            painter.circle_stroke(c, r.width() / 2.0, stroke);
            for dx in [-2.6, 2.6] {
                painter.line_segment(
                    [c + egui::vec2(dx, -3.5), c + egui::vec2(dx, 3.5)],
                    Stroke::new(2.0_f32, color),
                );
            }
        }
        NavIcon::Settings => {
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0;
                let dir = egui::vec2(a.cos(), a.sin());
                painter.line_segment(
                    [c + dir * (r.width() * 0.32), c + dir * (r.width() * 0.5)],
                    Stroke::new(2.2_f32, color),
                );
            }
            painter.circle_stroke(c, r.width() * 0.32, stroke);
            painter.circle_filled(c, 1.8, color);
        }
    }
}

/// App icons by exe name, read once and kept as textures. Apps whose icon cannot be read get a
/// lettered tile instead.
#[derive(Default)]
pub struct IconCache {
    textures: HashMap<String, Option<TextureHandle>>,
}

impl IconCache {
    pub fn get(
        &mut self,
        ctx: &egui::Context,
        process: &str,
        path: Option<&str>,
    ) -> Option<TextureHandle> {
        let key = process.to_ascii_lowercase();
        if let Some(found) = self.textures.get(&key) {
            // A rule for an app that was not running gets its icon once the app is seen.
            if found.is_some() || path.is_none() {
                return found.clone();
            }
        }
        let texture = path.and_then(|path| icons::exe_icon(path, 64)).map(|icon| {
            let size = icon.size as usize;
            let image = egui::ColorImage::from_rgba_unmultiplied([size, size], &icon.rgba);
            ctx.load_texture(format!("icon-{key}"), image, egui::TextureOptions::LINEAR)
        });
        self.textures.insert(key, texture.clone());
        texture
    }
}

/// An app's icon, or the first letter of its name on a tinted tile.
pub fn app_icon(ui: &mut Ui, texture: Option<&TextureHandle>, process: &str, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), Sense::hover());
    paint_app_icon(ui, rect, texture, process);
}

fn paint_app_icon(ui: &Ui, rect: Rect, texture: Option<&TextureHandle>, process: &str) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter();
    match texture {
        Some(texture) => {
            painter.image(
                texture.id(),
                rect,
                Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        None => {
            painter.rect_filled(rect, CornerRadius::same(6), tile_color(process));
            let letter = process
                .chars()
                .find(|ch| ch.is_alphanumeric())
                .unwrap_or('?')
                .to_uppercase()
                .to_string();
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                letter,
                egui::FontId::proportional(rect.height() * 0.55),
                Color32::WHITE,
            );
        }
    }
}

/// A steady color per app name, so a lettered tile is the same every time.
fn tile_color(process: &str) -> Color32 {
    let hash = process
        .to_ascii_lowercase()
        .bytes()
        .fold(2166136261_u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
    let hue = (hash % 360) as f32 / 360.0;
    egui::ecolor::Hsva::new(hue, 0.45, 0.55, 1.0).into()
}

/// What a row in the app list shows.
pub struct RowInfo<'a> {
    pub process: &'a str,
    pub subtitle: &'a str,
    pub icon: Option<&'a TextureHandle>,
    pub selected: bool,
    pub warning: Option<&'a str>,
    /// Some(on) for apps with a rule, drawn as a switch.
    pub enabled: Option<&'a mut bool>,
}

pub struct RowResponse {
    pub clicked: bool,
    pub toggled: bool,
}

/// One app in the list: icon, name, a line under it, and a switch for apps with a rule.
/// The row is clickable as a whole, and the switch keeps its own clicks.
pub fn app_row(ui: &mut Ui, info: &mut RowInfo<'_>) -> RowResponse {
    let width = ui.available_width();
    let (rect, row) = ui.allocate_exact_size(egui::vec2(width, 50.0), Sense::click());
    let row = row.on_hover_cursor(CursorIcon::PointingHand);
    let fill = if info.selected {
        ROW_SELECTED
    } else if row.hovered() {
        ROW_HOVER
    } else {
        Color32::TRANSPARENT
    };
    ui.painter()
        .add(Shape::rect_filled(rect, CornerRadius::same(10), fill));
    let icon_rect = Rect::from_min_size(
        egui::pos2(rect.left() + 10.0, rect.center().y - 14.0),
        egui::vec2(28.0, 28.0),
    );
    paint_app_icon(ui, icon_rect, info.icon, info.process);

    let switch_w = if info.enabled.is_some() {
        38.0 + 12.0
    } else {
        0.0
    };
    let warn_w = info
        .warning
        .map(|warning| text_width(ui, warning, 11.0) + 18.0)
        .unwrap_or(0.0);
    let text_left = icon_rect.right() + 10.0;
    let text_right = (rect.right() - 10.0 - switch_w - warn_w).max(text_left + 20.0);
    let text_rect = Rect::from_min_max(
        egui::pos2(text_left, rect.top() + 7.0),
        egui::pos2(text_right, rect.bottom() - 7.0),
    );
    let mut text_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(text_rect)
            .layout(Layout::top_down(Align::LEFT)),
    );
    text_ui.set_clip_rect(text_rect.intersect(ui.clip_rect()));
    text_ui.spacing_mut().item_spacing.y = 1.0;
    text_ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
    let dim = info.enabled.as_deref() == Some(&false);
    text_ui.label(
        RichText::new(info.process)
            .strong()
            .color(if dim { MUTED } else { TEXT }),
    );
    window_title(&mut text_ui, info.subtitle);

    if let Some(warning) = info.warning {
        let center = egui::pos2(
            rect.right() - 10.0 - switch_w - warn_w / 2.0,
            rect.center().y,
        );
        let pill = Rect::from_center_size(center, egui::vec2(warn_w - 6.0, 18.0));
        ui.painter()
            .rect_filled(pill, CornerRadius::same(9), WARN.gamma_multiply(0.2));
        ui.painter().text(
            center,
            egui::Align2::CENTER_CENTER,
            warning,
            egui::FontId::proportional(11.0),
            WARN,
        );
    }

    let mut toggled = false;
    if let Some(on) = info.enabled.as_deref_mut() {
        let switch_rect = Rect::from_min_size(
            egui::pos2(rect.right() - 10.0 - 38.0, rect.center().y - 11.0),
            egui::vec2(38.0, 22.0),
        );
        let mut switch_ui = ui.new_child(egui::UiBuilder::new().max_rect(switch_rect));
        toggled = switch(&mut switch_ui, on)
            .on_hover_text("Frost this app, or switch its rule off.")
            .changed();
    }
    RowResponse {
        clicked: row.clicked() && !toggled,
        toggled,
    }
}

/// Set in debug builds by BLURMAN_UI_REDACT, for screenshots that should not show what you
/// have open.
static REDACT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(debug_assertions)]
pub fn set_redact(on: bool) {
    REDACT.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// A window title under an app's name. When redacting, each word becomes a soft blob of the
/// same width, so the title keeps its shape and none of its letters.
pub fn window_title(ui: &mut Ui, title: &str) {
    let redact = REDACT.load(std::sync::atomic::Ordering::Relaxed) && title != "Not running";
    if !redact {
        ui.label(theme::muted(title).small());
        return;
    }
    let size = egui::TextStyle::Small.resolve(ui.style()).size;
    let space = text_width(ui, " ", size);
    let words: Vec<f32> = title
        .split_whitespace()
        .map(|word| text_width(ui, word, size))
        .collect();
    let total = words.iter().sum::<f32>() + space * words.len().saturating_sub(1) as f32;
    let width = total.min(ui.available_width());
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, size * 1.25), Sense::hover());
    let painter = ui
        .painter()
        .with_clip_rect(rect.expand(4.0).intersect(ui.clip_rect()));
    let mut x = rect.left();
    for word in words {
        let blob = Rect::from_min_size(
            egui::pos2(x, rect.center().y - size * 0.3),
            egui::vec2(word, size * 0.6),
        );
        // A faint halo around a firmer core reads as text seen through frosted glass.
        for (grow, alpha) in [(3.0_f32, 0.08), (1.5, 0.14), (0.0, 0.28)] {
            painter.rect_filled(
                blob.expand(grow),
                CornerRadius::same((size * 0.3 + grow) as u8),
                MUTED.gamma_multiply(alpha),
            );
        }
        x += word + space;
    }
}

/// A short message near the bottom of the window that fades out on its own.
pub fn toast(ctx: &egui::Context, text: &str, opacity: f32) {
    egui::Area::new(egui::Id::new("toast"))
        .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -24.0))
        .interactable(false)
        .order(egui::Order::Tooltip)
        .show(ctx, |ui| {
            ui.set_opacity(opacity);
            Frame::new()
                .fill(theme::POPUP)
                .stroke(Stroke::new(1.0_f32, Color32::from_white_alpha(40)))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(Margin::symmetric(16, 10))
                .shadow(egui::Shadow {
                    offset: [0, 6],
                    blur: 18,
                    spread: 0,
                    color: Color32::from_black_alpha(90),
                })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (dot, _) =
                            ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
                        ui.painter().circle_filled(dot.center(), 4.0, theme::OK);
                        ui.label(RichText::new(text).color(TEXT));
                    });
                });
        });
}

/// A banner across the top of a page.
pub fn banner(ui: &mut Ui, color: Color32, add_contents: impl FnOnce(&mut Ui)) {
    Frame::new()
        .fill(color.gamma_multiply(0.2))
        .stroke(Stroke::new(1.0_f32, color.gamma_multiply(0.5)))
        .corner_radius(10)
        .inner_margin(Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(add_contents);
        });
    ui.add_space(8.0);
}

/// A plain button drawn in the accent red, for things that cannot be undone.
pub fn danger_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text).color(Color32::from_rgb(255, 196, 204)))
        .fill(ACCENT.gamma_multiply(0.35))
        .stroke(Stroke::new(1.0_f32, ACCENT.gamma_multiply(0.8)))
}
