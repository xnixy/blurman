//! The controls the pages are built from, drawn the way Espalier and shellman draw theirs.

use super::panel::Island;
use crate::icons;
use crate::theme::{self, colors, mix, with_alpha, ISLAND_RADIUS, PILL_H};
use egui::accesskit::Role;
use egui::{
    Align, Color32, CornerRadius, CursorIcon, Frame, Layout, Margin, Pos2, Rect, Response,
    RichText, Sense, Stroke, StrokeKind, TextureHandle, Ui, UiBuilder, WidgetInfo, WidgetType,
};
use std::cell::RefCell;
use std::collections::HashMap;

pub const PILL_HEIGHT: f32 = PILL_H;

/// Inside a text field, so it is as tall as the buttons beside it.
pub const FIELD_MARGIN: Margin = Margin {
    left: 12,
    right: 12,
    top: 6,
    bottom: 6,
};

thread_local! {
    /// The islands drawn this frame, for the glass under them.
    static ISLANDS: RefCell<Vec<Island>> = const { RefCell::new(Vec::new()) };
}

/// The islands drawn since this was last asked.
pub fn take_islands() -> Vec<Island> {
    ISLANDS.with(|islands| std::mem::take(&mut *islands.borrow_mut()))
}

fn round(radius: f32) -> CornerRadius {
    CornerRadius::same(radius.round().clamp(0.0, 255.0) as u8)
}

/// Have glass put under `rect`, as much of it as shows.
fn on_glass(ui: &Ui, rect: Rect, radius: f32) {
    if !theme::composed() {
        return;
    }
    let shown = rect.intersect(ui.clip_rect());
    if shown.width() > 1.0 && shown.height() > 1.0 {
        ISLANDS.with(|islands| islands.borrow_mut().push(Island::new(shown, radius)));
    }
}

/// An island at `rect`: its own fill and line, on glass where there is glass.
pub fn island(ui: &Ui, rect: Rect, radius: f32) {
    let (fill, stroke) = theme::island();
    ui.painter()
        .rect(rect, round(radius), fill, stroke, StrokeKind::Inside);
    on_glass(ui, rect, radius);
}

/// An island as wide as the column, with a muted label for the group inside it.
pub fn card<R>(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    card_at_least(ui, label, 0.0, add).0
}

/// A card whose content is at least `min_height` tall, for cards that sit side by side.
/// Also gives back how tall the content is on its own.
pub fn card_at_least<R>(
    ui: &mut Ui,
    label: &str,
    min_height: f32,
    add: impl FnOnce(&mut Ui) -> R,
) -> (R, f32) {
    let behind = ui.painter().add(egui::Shape::Noop);
    let inner = Frame::new()
        .inner_margin(Margin::symmetric(22, 16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !label.is_empty() {
                group_label(ui, label);
            }
            let inner = add(ui);
            let own = ui.min_rect().height();
            if min_height > own {
                let mut rect = ui.min_rect();
                rect.set_height(min_height);
                ui.expand_to_include_rect(rect);
            }
            (inner, own)
        });
    island_behind(ui, behind, inner.response.rect);
    // With the item spacing, the 12 between islands.
    ui.add_space(4.0);
    inner.inner
}

/// Paint an island under content already laid out. Where a scrolling page cuts it off, what
/// still shows keeps its round corners, as the glass under it does.
fn island_behind(ui: &Ui, behind: egui::layers::ShapeIdx, rect: Rect) {
    let shown = rect.intersect(ui.clip_rect());
    if shown.width() <= 1.0 || shown.height() <= 1.0 {
        return;
    }
    let (fill, stroke) = theme::island();
    let radius = (ISLAND_RADIUS as f32).min(shown.height() / 2.0);
    ui.painter().set(
        behind,
        egui::epaint::RectShape::new(shown, round(radius), fill, stroke, StrokeKind::Inside),
    );
    on_glass(ui, shown, radius);
}

/// An island filling `rect`, its content kept inside it.
pub fn pane<R>(ui: &mut Ui, rect: Rect, add: impl FnOnce(&mut Ui) -> R) -> R {
    island(ui, rect, ISLAND_RADIUS as f32);
    let inner = rect.shrink(14.0);
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::top_down(Align::LEFT)),
    );
    child.set_clip_rect(inner.expand(2.0).intersect(ui.clip_rect()));
    add(&mut child)
}

/// The muted name of a group of settings, at the top of its island.
pub fn group_label(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(12.5).color(colors().muted));
    ui.add_space(2.0);
}

/// A hairline between rows of one island.
pub fn divider(ui: &mut Ui) {
    let c = colors();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 9.0), Sense::hover());
    let color = if c.high_contrast {
        c.text
    } else {
        with_alpha(c.text, 26)
    };
    ui.painter()
        .hline(rect.x_range(), rect.center().y, Stroke::new(1.0_f32, color));
}

/// Say what a painted control is, for screen readers. `selected` is its state, where it has one.
fn describe(ui: &Ui, response: &Response, role: Role, name: &str, selected: Option<bool>) {
    let enabled = ui.is_enabled();
    response.widget_info(|| match selected {
        Some(on) => WidgetInfo::selected(WidgetType::Checkbox, enabled, on, name),
        None => WidgetInfo::labeled(WidgetType::Button, enabled, name),
    });
    ui.ctx()
        .accesskit_node_builder(response.id, |node| node.set_role(role));
}

/// Give a text field, slider, or number field its name, for screen readers.
pub fn name(ui: &Ui, response: &Response, name: &str) {
    ui.ctx()
        .accesskit_node_builder(response.id, |node| node.set_label(name));
}

/// The ring around a painted control that has the keyboard focus.
fn focus_ring(ui: &Ui, response: &Response, rect: Rect, radius: f32) {
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            round(radius + 2.0),
            Stroke::new(2.0_f32, colors().accent_text),
            StrokeKind::Outside,
        );
    }
}

/// A glyph from Segoe Fluent Icons, and the plain character that stands in without that font.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Icon(pub char, pub &'static str);

impl Icon {
    pub const CLOSE: Icon = Icon('\u{E8BB}', "×");
    pub const MAXIMIZE: Icon = Icon('\u{E922}', "□");
    pub const RESTORE: Icon = Icon('\u{E923}', "❐");
    pub const MINIMIZE: Icon = Icon('\u{E921}', "–");
    pub const MENU: Icon = Icon('\u{E700}', "≡");
    pub const BACK: Icon = Icon('\u{E72B}', "‹");
    pub const PICK: Icon = Icon('\u{E7C9}', "+");
    pub const RESTORE_PRESETS: Icon = Icon('\u{E72C}', "↻");
    pub const PAUSE: Icon = Icon('\u{E769}', "‖");
    pub const PLAY: Icon = Icon('\u{E768}', "▶");
    pub const APPS: Icon = Icon('\u{E71D}', "A");
    pub const PRESETS: Icon = Icon('\u{E790}', "P");
    pub const SHORTCUTS: Icon = Icon('\u{E765}', "K");
    pub const AUTO_PAUSE: Icon = Icon('\u{E823}', "T");
    pub const SETTINGS: Icon = Icon('\u{E713}', "S");

    pub fn paint(self, painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
        let (text, font) = if theme::has_icons() {
            (self.0.to_string(), theme::icon_font(size))
        } else {
            (self.1.to_string(), egui::FontId::proportional(size + 2.0))
        };
        painter.text(center, egui::Align2::CENTER_CENTER, text, font, color);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pill {
    Primary,
    Secondary,
    Danger,
}

fn pill(ui: &mut Ui, text: &str, kind: Pill, min_width: f32) -> Response {
    let c = colors();
    let font = if kind == Pill::Primary {
        theme::semibold(13.0)
    } else {
        egui::FontId::proportional(13.0)
    };
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER);
    let width = (galley.size().x + 28.0)
        .max(min_width)
        .min(ui.available_width().max(40.0));
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, PILL_H), Sense::click());
    describe(ui, &response, Role::Button, text, None);
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let enabled = ui.is_enabled();
    let hot = enabled && (response.hovered() || response.has_focus());
    let (fill, stroke, ink) = match kind {
        _ if c.high_contrast => {
            if hot || kind == Pill::Primary {
                (c.accent, Stroke::new(1.0_f32, c.text), c.on_accent)
            } else {
                (c.island, Stroke::new(1.0_f32, c.text), c.text)
            }
        }
        Pill::Primary => (
            if hot {
                mix(c.accent, c.text, 0.12)
            } else {
                c.accent
            },
            Stroke::NONE,
            c.on_accent,
        ),
        Pill::Secondary => (
            if hot { c.control_hover } else { c.soft },
            Stroke::new(1.0_f32, with_alpha(c.text, 44)),
            c.text,
        ),
        Pill::Danger => (
            with_alpha(c.danger, if hot { 90 } else { 50 }),
            Stroke::new(1.0_f32, with_alpha(c.danger, 200)),
            c.danger_text,
        ),
    };
    // High contrast has its own color for what is switched off, and no fading.
    let (fade, ink) = match (enabled, c.high_contrast) {
        (true, _) => (1.0, ink),
        (false, true) => (1.0, c.muted),
        (false, false) => (0.6, c.muted),
    };
    let mut painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    // A pill that is switched off is dimmed here. egui's own fade for disabled widgets,
    // on top of that, loses it altogether on a white island.
    painter.set_fade_to_color(None);
    painter.rect(
        rect,
        round(PILL_H / 2.0),
        fill.gamma_multiply(fade),
        stroke,
        StrokeKind::Inside,
    );
    let at = rect.center() - galley.size() / 2.0;
    painter.galley(at, galley, ink.gamma_multiply(fade));
    focus_ring(ui, &response, rect, PILL_H / 2.0);
    if enabled {
        response.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        response
    }
}

/// A pill at least `min_width` wide: the accent one, or a soft one.
pub fn sized(ui: &mut Ui, text: &str, primary: bool, min_width: f32) -> Response {
    let kind = if primary {
        Pill::Primary
    } else {
        Pill::Secondary
    };
    pill(ui, text, kind, min_width)
}

/// The one thing to do on a view: an accent pill.
pub fn primary(ui: &mut Ui, text: &str) -> Response {
    pill(ui, text, Pill::Primary, 88.0)
}

/// Anything else to do: a soft pill.
pub fn secondary(ui: &mut Ui, text: &str) -> Response {
    pill(ui, text, Pill::Secondary, 80.0)
}

/// Something that cannot be undone.
pub fn danger(ui: &mut Ui, text: &str) -> Response {
    pill(ui, text, Pill::Danger, 80.0)
}

/// A round button with only a glyph. `name` is its tooltip and what a screen reader calls it.
pub fn round_icon(ui: &mut Ui, icon: Icon, name: &str) -> Response {
    round_icon_sized(ui, icon, name, 13.0, false)
}

/// Minimize, maximize, and close: smaller glyphs, and close turns red.
pub fn window_button(ui: &mut Ui, icon: Icon, name: &str) -> Response {
    round_icon_sized(ui, icon, name, 10.0, icon == Icon::CLOSE)
}

fn round_icon_sized(ui: &mut Ui, icon: Icon, name: &str, size: f32, closes: bool) -> Response {
    let c = colors();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(30.0, 30.0), Sense::click());
    describe(ui, &response, Role::Button, name, None);
    if ui.is_rect_visible(rect) {
        let enabled = ui.is_enabled();
        let hot = enabled && (response.hovered() || response.has_focus());
        let mut ink = if enabled { c.text } else { c.muted };
        if hot {
            let fill = if c.high_contrast {
                ink = c.on_accent;
                c.accent
            } else if closes {
                ink = Color32::WHITE;
                Color32::from_rgb(196, 43, 28)
            } else {
                c.control_hover
            };
            ui.painter().circle_filled(rect.center(), 15.0, fill);
        }
        icon.paint(ui.painter(), rect.center(), size, ink);
        focus_ring(ui, &response, rect, 15.0);
    }
    response.on_hover_text(name)
}

/// An on/off switch. `name` is what a screen reader calls it.
pub fn switch(ui: &mut Ui, on: &mut bool, name: &str) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(egui::vec2(36.0, 20.0), Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    describe(ui, &response, Role::Switch, name, Some(*on));
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_responsive(response.id, *on);
        let c = colors();
        let fade = if ui.is_enabled() { 1.0 } else { 0.45 };
        let painter = ui.painter();
        let knob_at = egui::pos2(
            egui::lerp((rect.left() + 10.0)..=(rect.right() - 10.0), t),
            rect.center().y,
        );
        if c.high_contrast {
            // Outlined when off, filled when on: the state shows without relying on a tint.
            let (fill, knob) = if *on {
                (c.accent, c.on_accent)
            } else {
                (c.island, c.text)
            };
            painter.rect(
                rect,
                round(10.0),
                fill,
                Stroke::new(1.0_f32, c.text),
                StrokeKind::Inside,
            );
            painter.circle_filled(knob_at, 6.0, knob);
        } else {
            let track = mix(mix(c.muted, c.popup, 0.55), c.accent, t);
            // On an accent that is light, a white knob would vanish.
            let knob = mix(c.muted, c.on_accent, t);
            painter.rect_filled(rect, round(10.0), track.gamma_multiply(fade));
            painter.circle_filled(knob_at, 7.0, knob.gamma_multiply(fade));
        }
        focus_ring(ui, &response, rect, 10.0);
    }
    response.on_hover_cursor(CursorIcon::PointingHand)
}

/// A row with its control on the right. The control is laid out first and the text wraps in
/// what is left, so neither ends up under the other.
pub fn trailing<R>(
    ui: &mut Ui,
    control: impl FnOnce(&mut Ui) -> R,
    body: impl FnOnce(&mut Ui),
) -> R {
    let width = ui.available_width();
    let top = ui.cursor().min;
    // The control is centered on the text beside it, whose height is only known once it has
    // wrapped: the height it had on the last frame is used.
    let id = ui.auto_id_with("trailing");
    let last: f32 = ui.ctx().data(|data| data.get_temp(id)).unwrap_or(PILL_H);
    let row = Rect::from_min_size(top, egui::vec2(width, last.max(PILL_H)));
    let mut control_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(row)
            .layout(Layout::right_to_left(Align::Center)),
    );
    let result = control(&mut control_ui);
    let control_rect = control_ui.min_rect();
    let left = (control_rect.left() - 14.0 - top.x).max(0.0);
    let mut body_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_size(top, egui::vec2(left, f32::INFINITY)))
            .layout(Layout::top_down(Align::LEFT)),
    );
    body_ui.set_width(left);
    body_ui.spacing_mut().item_spacing.y = 2.0;
    // Text shorter than the control sits level with it.
    let text_top = body_ui.cursor().top();
    let slack = ((PILL_H - last) / 2.0).max(0.0);
    body_ui.add_space(slack);
    body(&mut body_ui);
    let height = body_ui.min_rect().bottom() - text_top - slack;
    if (height - last).abs() > 0.5 {
        ui.ctx().data_mut(|data| data.insert_temp(id, height));
        ui.ctx().request_repaint();
    }
    let used = Rect::from_min_size(
        top,
        egui::vec2(
            width,
            (height + slack * 2.0)
                .max(control_rect.height())
                .max(PILL_H),
        ),
    );
    ui.allocate_rect(used, Sense::hover());
    result
}

/// A setting: its title and a muted line about it on the left, its control on the right.
pub fn setting_row<R>(
    ui: &mut Ui,
    title: &str,
    detail: &str,
    control: impl FnOnce(&mut Ui) -> R,
) -> R {
    trailing(ui, control, |ui| {
        ui.add(egui::Label::new(RichText::new(title).color(colors().text)).wrap());
        if !detail.is_empty() {
            ui.add(egui::Label::new(theme::detail(detail)).wrap());
        }
    })
}

/// A setting that is a switch. True when it was flipped.
pub fn switch_row(ui: &mut Ui, on: &mut bool, title: &str, detail: &str) -> bool {
    setting_row(ui, title, detail, |ui| switch(ui, on, title).changed())
}

/// A choice among a few: a chip, filled with the accent when it is the current one.
pub fn chip(ui: &mut Ui, selected: bool, label: &str) -> Response {
    let c = colors();
    let font = if selected {
        theme::semibold(13.0)
    } else {
        egui::FontId::proportional(13.0)
    };
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, Color32::PLACEHOLDER);
    let size = egui::vec2((galley.size().x + 24.0).max(64.0), 26.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    describe(ui, &response, Role::RadioButton, label, Some(selected));
    if ui.is_rect_visible(rect) {
        let hot = response.hovered() || response.has_focus();
        let (fill, stroke, ink) = if selected {
            (c.accent, Stroke::NONE, c.on_accent)
        } else if c.high_contrast {
            if hot {
                (c.accent, Stroke::new(1.0_f32, c.text), c.on_accent)
            } else {
                (c.island, Stroke::new(1.0_f32, c.text), c.text)
            }
        } else {
            (
                if hot { c.control_hover } else { c.control },
                Stroke::new(1.0_f32, with_alpha(c.text, 44)),
                c.text,
            )
        };
        let fade = if ui.is_enabled() { 1.0 } else { 0.45 };
        ui.painter().rect(
            rect,
            round(13.0),
            fill.gamma_multiply(fade),
            stroke,
            StrokeKind::Inside,
        );
        let at = rect.center() - galley.size() / 2.0;
        ui.painter().galley(at, galley, ink.gamma_multiply(fade));
        focus_ring(ui, &response, rect, 13.0);
    }
    response.on_hover_cursor(CursorIcon::PointingHand)
}

/// One choice out of a few, as chips that wrap. Returns whether the choice changed.
pub fn segmented<T: Copy + PartialEq>(
    ui: &mut Ui,
    value: &mut T,
    options: &[(T, &str, &str)],
) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        changed = segmented_in(ui, value, options);
    });
    changed
}

/// The chips of `segmented`, added to a row that is already laid out.
pub fn segmented_in<T: Copy + PartialEq>(
    ui: &mut Ui,
    value: &mut T,
    options: &[(T, &str, &str)],
) -> bool {
    let mut changed = false;
    for &(option, label, tip) in options {
        let selected = *value == option;
        let mut response = chip(ui, selected, label);
        if !tip.is_empty() {
            response = response.on_hover_text(tip);
        }
        if response.clicked() && !selected {
            *value = option;
            changed = true;
        }
    }
    changed
}

/// A round color to choose, ringed when it is the current one.
pub fn swatch(ui: &mut Ui, color: Color32, selected: bool, name: &str) -> Response {
    let c = colors();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), Sense::click());
    describe(ui, &response, Role::RadioButton, name, Some(selected));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.circle_filled(rect.center(), 10.0, color);
        if c.high_contrast {
            painter.circle_stroke(rect.center(), 10.0, Stroke::new(1.0_f32, c.text));
        }
        if selected || response.hovered() {
            let ring = if selected { c.text } else { c.muted };
            painter.circle_stroke(rect.center(), 13.0, Stroke::new(2.0_f32, ring));
        }
        focus_ring(ui, &response, rect, 14.0);
    }
    response
        .on_hover_text(name)
        .on_hover_cursor(CursorIcon::PointingHand)
}

fn paint_tag(ui: &Ui, rect: Rect, galley: std::sync::Arc<egui::Galley>, color: Color32) {
    ui.painter().rect_stroke(
        rect,
        round(10.0),
        Stroke::new(1.0_f32, color),
        StrokeKind::Inside,
    );
    let at = rect.center() - galley.size() / 2.0;
    ui.painter().galley(at, galley, color);
}

/// What an empty pane shows: a large muted glyph and one line.
pub fn empty(ui: &mut Ui, icon: Icon, text: &str) {
    ui.vertical_centered(|ui| {
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 44.0), Sense::hover());
        icon.paint(ui.painter(), rect.center(), 34.0, colors().muted);
        ui.add(egui::Label::new(theme::muted(text)).wrap());
    });
}

/// A label on the left and a slider filling the rest of the row, its number kept inside.
pub fn slider_row<N: egui::emath::Numeric>(
    ui: &mut Ui,
    label: &str,
    value: &mut N,
    range: std::ops::RangeInclusive<N>,
    suffix: &str,
) -> bool {
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
            // The number is a field of its own, not the slider's, so that it can be named
            // for a screen reader.
            let slider = ui.add(egui::Slider::new(value, range.clone()).show_value(false));
            name(ui, &slider, label);
            let number = ui.add(egui::DragValue::new(value).range(range).suffix(suffix));
            name(ui, &number, &format!("{label}, as a number"));
            changed = slider.changed() || number.changed();
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

pub fn text_width_in(ui: &Ui, text: &str, font: egui::FontId) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_owned(), font, Color32::WHITE)
        .size()
        .x
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
                .margin(FIELD_MARGIN)
                .desired_width((ui.available_width() - 24.0).max(20.0)),
        );
        name(ui, &field, label);
        done = field.lost_focus();
    });
    done
}

/// "HH:MM", in the warning color while it does not read as a time. True once the field is
/// left, by Enter or a click elsewhere: a time is not saved one keystroke at a time.
pub fn clock_field(ui: &mut Ui, id: &str, label: &str, text: &mut String) -> bool {
    let valid = crate::rules::parse_clock(text).is_some();
    let mut edit = egui::TextEdit::singleline(text)
        .id(egui::Id::new(id))
        .margin(FIELD_MARGIN)
        .desired_width(44.0)
        .hint_text("HH:MM");
    if !valid {
        edit = edit.text_color(colors().warn);
    }
    let field = ui.add(edit);
    name(ui, &field, label);
    field.lost_focus()
}

/// One page in the sidebar: a glyph and its name, or only the glyph in the rail.
pub fn nav_row(ui: &mut Ui, icon: Icon, label: &str, selected: bool, rail: bool) -> Response {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 36.0), Sense::click());
    describe(ui, &response, Role::Tab, label, Some(selected));
    if ui.is_rect_visible(rect) {
        let c = colors();
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        let hot = response.hovered() || response.has_focus();
        let ink = if selected {
            painter.rect_filled(rect, round(12.0), c.accent);
            c.on_accent
        } else if hot && c.high_contrast {
            painter.rect_stroke(
                rect,
                round(12.0),
                Stroke::new(1.0_f32, c.text),
                StrokeKind::Inside,
            );
            c.text
        } else {
            if hot {
                painter.rect_filled(rect, round(12.0), c.row_hover);
            }
            c.text
        };
        if rail {
            icon.paint(&painter, rect.center(), 16.0, ink);
        } else {
            let glyph_at = egui::pos2(rect.left() + 22.0, rect.center().y);
            icon.paint(&painter, glyph_at, 16.0, ink);
            let font = if selected {
                theme::semibold(14.0)
            } else {
                egui::FontId::proportional(14.0)
            };
            painter.text(
                egui::pos2(rect.left() + 42.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                label,
                font,
                ink,
            );
        }
        focus_ring(ui, &response, rect, 12.0);
    }
    let response = response.on_hover_cursor(CursorIcon::PointingHand);
    if rail {
        response.on_hover_text(label)
    } else {
        response
    }
}

/// App icons by exe name, read once and kept as textures. Apps whose icon cannot be read get a
/// lettered tile instead.
#[derive(Default)]
pub struct IconCache {
    /// By exe name: the path the icon was read from, and the icon if that worked.
    textures: HashMap<String, (Option<String>, Option<TextureHandle>)>,
}

impl IconCache {
    pub fn get(
        &mut self,
        ctx: &egui::Context,
        process: &str,
        path: Option<&str>,
    ) -> Option<TextureHandle> {
        let key = process.to_ascii_lowercase();
        if let Some((tried, found)) = self.textures.get(&key) {
            // A rule for an app that was not running gets its icon once the app is seen. An exe
            // whose icon could not be read is not read again on every frame.
            if found.is_some() || path.is_none() || tried.as_deref() == path {
                return found.clone();
            }
        }
        let texture = path.and_then(|path| icons::exe_icon(path, 64)).map(|icon| {
            let size = icon.size as usize;
            let image = egui::ColorImage::from_rgba_unmultiplied([size, size], &icon.rgba);
            ctx.load_texture(format!("icon-{key}"), image, egui::TextureOptions::LINEAR)
        });
        self.textures
            .insert(key, (path.map(str::to_string), texture.clone()));
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
            if colors().high_contrast {
                painter.rect_stroke(
                    rect,
                    CornerRadius::same(6),
                    Stroke::new(1.0_f32, colors().text),
                    StrokeKind::Inside,
                );
            }
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
    let said = if redacting() && info.subtitle != "Not running" {
        info.process.to_string()
    } else {
        format!("{}, {}", info.process, info.subtitle)
    };
    describe(ui, &row, Role::ListItem, &said, Some(info.selected));
    let c = colors();
    if info.selected && c.high_contrast {
        // Ordinary text over a highlight fill is not a pair Windows allows.
        ui.painter().rect_stroke(
            rect,
            round(12.0),
            Stroke::new(2.0_f32, c.accent),
            StrokeKind::Inside,
        );
    } else if info.selected {
        ui.painter().rect_filled(rect, round(12.0), c.row_selected);
    } else if row.hovered() {
        ui.painter().rect_filled(rect, round(12.0), c.row_hover);
    }
    focus_ring(ui, &row, rect.shrink(2.0), 10.0);
    let icon_rect = Rect::from_min_size(
        egui::pos2(rect.left() + 10.0, rect.center().y - 14.0),
        egui::vec2(28.0, 28.0),
    );
    paint_app_icon(ui, icon_rect, info.icon, info.process);

    let switch_w = if info.enabled.is_some() {
        36.0 + 12.0
    } else {
        0.0
    };
    let warning = info.warning.map(|warning| {
        ui.painter().layout_no_wrap(
            warning.to_owned(),
            egui::FontId::proportional(11.0),
            Color32::PLACEHOLDER,
        )
    });
    let warn_w = warning
        .as_ref()
        .map(|galley| galley.size().x + 14.0 + 8.0)
        .unwrap_or(0.0);
    let text_left = icon_rect.right() + 10.0;
    let text_right = (rect.right() - 10.0 - switch_w - warn_w).max(text_left + 20.0);
    let text_rect = Rect::from_min_max(
        egui::pos2(text_left, rect.top() + 7.0),
        egui::pos2(text_right, rect.bottom() - 7.0),
    );
    let mut text_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(text_rect)
            .layout(Layout::top_down(Align::LEFT)),
    );
    text_ui.set_clip_rect(text_rect.intersect(ui.clip_rect()));
    text_ui.spacing_mut().item_spacing.y = 1.0;
    text_ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
    let dim = info.enabled.as_deref() == Some(&false);
    text_ui.label(
        RichText::new(info.process)
            .size(13.5)
            .color(if dim { c.muted } else { c.text }),
    );
    window_title(&mut text_ui, info.subtitle, info.selected);

    if let Some(galley) = warning {
        let pill = Rect::from_center_size(
            egui::pos2(
                rect.right() - 10.0 - switch_w - warn_w / 2.0,
                rect.center().y,
            ),
            egui::vec2(warn_w - 8.0, 20.0),
        );
        paint_tag(ui, pill, galley, c.warn);
    }

    let mut toggled = false;
    if let Some(on) = info.enabled.as_deref_mut() {
        let switch_rect = Rect::from_min_size(
            egui::pos2(rect.right() - 10.0 - 36.0, rect.center().y - 10.0),
            egui::vec2(36.0, 20.0),
        );
        let mut switch_ui = ui.new_child(UiBuilder::new().max_rect(switch_rect));
        toggled = switch(&mut switch_ui, on, &format!("Frost {}", info.process))
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

fn redacting() -> bool {
    REDACT.load(std::sync::atomic::Ordering::Relaxed)
}

/// A window title under an app's name. When redacting, each word becomes a soft blob of the
/// same width, so the title keeps its shape and none of its letters.
pub fn window_title(ui: &mut Ui, title: &str, selected: bool) {
    if !redacting() || title == "Not running" {
        let mut text = theme::detail(title);
        if selected && !colors().high_contrast {
            // Muted text is lost on the selection's wash.
            text = text.color(with_alpha(colors().text, 200));
        }
        ui.label(text);
        return;
    }
    let size = 11.5;
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
                round(size * 0.3 + grow),
                colors().muted.gamma_multiply(alpha),
            );
        }
        x += word + space;
    }
}

/// A short message near the bottom of the window that fades out on its own.
pub fn toast(ctx: &egui::Context, text: &str, opacity: f32) {
    egui::Area::new(egui::Id::new("toast"))
        .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -28.0))
        .interactable(false)
        .order(egui::Order::Tooltip)
        .show(ctx, |ui| {
            ui.set_opacity(opacity);
            let c = colors();
            let shadow = if c.high_contrast {
                egui::Shadow::NONE
            } else {
                egui::Shadow {
                    offset: [0, 6],
                    blur: 18,
                    spread: 0,
                    color: Color32::from_black_alpha(if c.light { 40 } else { 90 }),
                }
            };
            Frame::new()
                .fill(c.popup)
                .stroke(Stroke::new(
                    1.0_f32,
                    if c.high_contrast { c.text } else { c.hairline },
                ))
                .corner_radius(CornerRadius::same(18))
                .inner_margin(Margin::symmetric(16, 9))
                .shadow(shadow)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (dot, _) =
                            ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
                        ui.painter().circle_filled(dot.center(), 4.0, c.ok);
                        ui.label(RichText::new(text).color(c.text));
                    });
                });
        });
}

/// A note that applies to every page: an island across the top, its buttons on the right.
pub fn banner(ui: &mut Ui, buttons: impl FnOnce(&mut Ui), text: impl FnOnce(&mut Ui)) {
    let behind = ui.painter().add(egui::Shape::Noop);
    let inner = Frame::new()
        .inner_margin(Margin::symmetric(18, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            trailing(ui, buttons, text);
        });
    island_behind(ui, behind, inner.response.rect);
    ui.add_space(4.0);
}
