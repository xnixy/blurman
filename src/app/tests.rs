use super::apps::{finish_controls, preset_summary};
use super::widgets::{self, RowInfo};
use super::{fit_frame_in, fit_in, keep_unsaved_looks, resize_edge};
use crate::mapping::{BlurStyle, Finish, FocusLook, Look, TintSource};
use crate::rules::{self, Store};
use crate::theme::{self, Mode};
use egui::{Slider, Ui};
use windows::Win32::Foundation::RECT;

fn look(transparency: u8, blur: u8, style: BlurStyle) -> Look {
    Look {
        transparency,
        blur,
        style,
        ..Look::default()
    }
}

/// How far `add` paints outside a region `width` points wide.
fn sticks_out(width: f32, add: impl FnOnce(&mut Ui)) -> f32 {
    let ctx = egui::Context::default();
    theme::install_fonts(&ctx);
    theme::restyle(&ctx);
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(width, 800.0),
        )),
        ..Default::default()
    };
    let mut add = Some(add);
    let mut overflow = 0.0_f32;
    let _ = ctx.run(raw, |ctx| {
        egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show(ctx, |ui| {
                let bounds = ui.max_rect();
                let inner = ui.scope(|ui| {
                    if let Some(add) = add.take() {
                        add(ui);
                    }
                });
                let rect = inner.response.rect;
                overflow = (rect.right() - bounds.right())
                    .max(bounds.left() - rect.left())
                    .max(0.0);
            });
    });
    overflow
}

#[test]
fn an_unsaved_drag_survives_a_reload_and_the_file_wins_the_rest() {
    let mut local = Store::default();
    local.upsert(rules::new_rule("notepad", look(55, 70, BlurStyle::Acrylic)));
    local.upsert(rules::new_rule("gone", look(30, 40, BlurStyle::Frost)));
    // The tray paused everything from an older copy of the file, and the CLI added chrome.
    let mut disk = Store {
        paused: true,
        ..Store::default()
    };
    let mut notepad = rules::new_rule("notepad", look(30, 40, BlurStyle::Frost));
    notepad.focus = FocusLook::Solid;
    disk.upsert(notepad);
    disk.upsert(rules::new_rule("chrome", look(20, 10, BlurStyle::Frost)));
    keep_unsaved_looks(&mut disk, &local);
    assert!(disk.paused);
    assert_eq!(disk.rules.len(), 2);
    let notepad = disk.rule("notepad").unwrap();
    assert_eq!(notepad.look, look(55, 70, BlurStyle::Acrylic));
    // Behavior is written at once, so the file is right about it.
    assert_eq!(notepad.focus, FocusLook::Solid);
    assert_eq!(disk.rule("chrome").unwrap().look.transparency, 20);
    assert!(disk.rule("gone").is_none());
}

#[test]
fn slider_rows_stay_inside_a_narrow_editor() {
    let overflow = sticks_out(320.0, |ui| {
        let mut transparency = 70_u8;
        let mut blur = 100_u8;
        widgets::slider_row(ui, "Transparency", &mut transparency, 10..=70, "%");
        widgets::slider_row(ui, "Milkiness", &mut blur, 1..=100, "");
    });
    assert!(overflow <= 1.0, "sliders stick out by {overflow}px");
}

#[test]
fn finish_controls_stay_inside_a_narrow_editor() {
    let overflow = sticks_out(380.0, |ui| {
        let mut look = Look::default();
        finish_controls(ui, &mut look, false);
    });
    assert!(overflow <= 1.0, "finish controls stick out by {overflow}px");
}

#[test]
fn app_rows_keep_long_names_and_their_switch_inside_the_list() {
    let overflow = sticks_out(280.0, |ui| {
        let mut on = true;
        widgets::app_row(
            ui,
            &mut RowInfo {
                process: "ApplicationFrameHost.exe with a very long name indeed",
                subtitle: "A very long window title that should stay inside the row",
                icon: None,
                selected: true,
                warning: Some("Fullscreen"),
                enabled: Some(&mut on),
            },
        );
    });
    assert!(overflow <= 1.0, "app row sticks out by {overflow}px");
}

#[test]
fn text_rows_fill_the_row_without_spilling() {
    let overflow = sticks_out(300.0, |ui| {
        let mut text = "YouTube | Netflix | a very long pattern that keeps going".to_string();
        widgets::text_row(ui, "Skip titles", &mut text, "hint");
    });
    assert!(overflow <= 1.0, "title field sticks out by {overflow}px");
}

fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
    RECT {
        left,
        top,
        right,
        bottom,
    }
}

#[test]
fn windows_are_brought_fully_onto_their_monitor() {
    // A 4K monitor's work area, above the taskbar.
    let work = rect(0, 0, 3840, 2088);
    // Already inside: left alone.
    let inside = rect(400, 300, 2020, 1380);
    assert_eq!(fit_in(inside, work), inside);
    // Hanging off the right and bottom: moved back, same size.
    assert_eq!(
        fit_in(rect(3000, 1500, 4620, 2580), work),
        rect(2220, 1008, 3840, 2088)
    );
    // Saved on a monitor that is gone, far to the left: back on this one.
    assert_eq!(
        fit_in(rect(-5000, -900, -3380, 180), work),
        rect(0, 0, 1620, 1080)
    );
    // Taller and wider than the screen, as the growing bug left it: shrunk to fit.
    let fitted = fit_in(rect(1556, 0, 3849, 2097), work);
    assert!(fitted.left >= work.left && fitted.right <= work.right);
    assert!(fitted.top >= work.top && fitted.bottom <= work.bottom);
    assert_eq!(fitted.bottom - fitted.top, 2088 * 9 / 10);
}

#[test]
fn a_snapped_window_is_not_shrunk_for_its_invisible_borders() {
    let work = rect(0, 0, 3840, 2088);
    // Snapped to the left half: the frame fills it, the window's own rectangle hangs over.
    let frame = rect(0, 0, 1920, 2088);
    let window = rect(-11, 0, 1931, 2099);
    assert_eq!(fit_frame_in(window, frame, work), window);
    // Really off the right edge: moved back by what shows, borders kept.
    let frame = rect(3000, 100, 4620, 1180);
    let window = rect(2989, 100, 4631, 1191);
    assert_eq!(
        fit_frame_in(window, frame, work),
        rect(2209, 100, 3851, 1191)
    );
    // No frame to go by: the window's rectangle is used, as before.
    let window = rect(3000, 1500, 4620, 2580);
    assert_eq!(fit_frame_in(window, window, work), fit_in(window, work));
    assert_eq!(
        fit_frame_in(window, rect(0, 0, 10, 10), work),
        fit_in(window, work)
    );
}

#[test]
fn preset_summaries_mention_only_what_differs() {
    assert_eq!(
        preset_summary(&look(20, 30, BlurStyle::Frost)),
        "Frost · 20% see-through · blur 30"
    );
    let vibrant = Look {
        finish: Finish {
            saturation: 170,
            grain: 25,
            ..Finish::default()
        },
        ..look(40, 60, BlurStyle::Frost)
    };
    assert_eq!(
        preset_summary(&vibrant),
        "Frost · 40% see-through · blur 60 · saturation 170% · grain 25"
    );
    let accent = Look {
        finish: Finish {
            tint_source: TintSource::Accent,
            ..Finish::default()
        },
        ..look(40, 60, BlurStyle::Acrylic)
    };
    assert_eq!(
        preset_summary(&accent),
        "Acrylic · 40% see-through · milkiness 60 · accent tint"
    );
    let light = rules::default_presets()
        .into_iter()
        .find(|preset| preset.name == "Light apps")
        .unwrap();
    assert_eq!(
        preset_summary(&light.look),
        "Frost · 35% see-through · blur 90 · saturation 135% · milk 85% · grain 8"
    );
}

#[test]
fn both_palettes_keep_text_readable_on_their_cards() {
    for light in [false, true] {
        let c = theme::set(light, "Blue", Mode::Solid, false);
        assert_eq!(c.light, light);
        // Text against the page it sits on: dark on light, light on dark.
        let luma = |color: egui::Color32| {
            0.2126 * color.r() as f32 + 0.7152 * color.g() as f32 + 0.0722 * color.b() as f32
        };
        let gap = (luma(c.text) - luma(c.bg)).abs();
        assert!(gap > 150.0, "text and page are too close in light={light}");
        assert_eq!(luma(c.text) < luma(c.bg), light);
    }
}

#[test]
fn every_accent_keeps_its_fill_and_the_text_on_it_apart() {
    use crate::app::look::contrast;
    for (name, ..) in theme::ACCENTS {
        for light in [false, true] {
            let c = theme::set(light, name, Mode::Solid, false);
            assert!(
                contrast(c.on_accent, c.accent) >= 3.0,
                "text on the {name} accent is hard to read in light={light}"
            );
            assert!(
                contrast(c.accent, c.bg) >= 1.6,
                "the {name} accent hardly shows on the page in light={light}"
            );
        }
    }
}

#[test]
fn the_window_resizes_from_a_band_along_its_edges() {
    use egui::ResizeDirection::*;
    let window = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let at = |x: f32, y: f32| resize_edge(window, egui::pos2(x, y));
    assert_eq!(at(400.0, 2.0), Some(North));
    assert_eq!(at(400.0, 597.0), Some(South));
    assert_eq!(at(3.0, 300.0), Some(West));
    assert_eq!(at(798.0, 300.0), Some(East));
    // Corners reach further along the edge than the band is deep.
    assert_eq!(at(10.0, 3.0), Some(NorthWest));
    assert_eq!(at(3.0, 10.0), Some(NorthWest));
    assert_eq!(at(795.0, 590.0), Some(SouthEast));
    assert_eq!(at(790.0, 2.0), Some(NorthEast));
    assert_eq!(at(4.0, 592.0), Some(SouthWest));
    // The gap between the edge and the islands is wider than the band.
    assert_eq!(at(8.0, 300.0), None);
    assert_eq!(at(400.0, 300.0), None);
    assert_eq!(at(-5.0, 300.0), None);
}

#[test]
fn setting_rows_wrap_their_text_beside_the_control() {
    let overflow = sticks_out(260.0, |ui| {
        let mut on = true;
        widgets::switch_row(
            ui,
            &mut on,
            "Keep running in the tray when the window closes",
            "Apps stay frosted. Click the tray icon to come back, or right-click it to pause.",
        );
        widgets::setting_row(
            ui,
            "Delete every rule",
            "Every app goes back to normal.",
            |ui| widgets::danger(ui, "Delete all rules…"),
        );
    });
    assert!(overflow <= 1.0, "setting rows stick out by {overflow}px");
}
