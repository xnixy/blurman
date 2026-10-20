use super::apps::{finish_controls, preset_summary};
use super::widgets::{self, RowInfo};
use super::{fit_in, keep_unsaved_looks};
use crate::mapping::{BlurStyle, Finish, FocusLook, Look, TintSource};
use crate::rules::{self, Store};
use crate::theme;
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
    theme::apply(&ctx);
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
        widgets::slider_row(ui, "Transparency", |ui| {
            ui.add(Slider::new(&mut transparency, 10..=70).suffix("%"))
        });
        widgets::slider_row(ui, "Milkiness", |ui| {
            ui.add(Slider::new(&mut blur, 1..=100))
        });
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
}
