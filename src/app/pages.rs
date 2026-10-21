//! Presets, Shortcuts, Auto-pause, and Settings.

use super::apps::preset_summary;
use super::widgets;
use super::{BlurmanApp, Confirm};
use crate::autostart;
use crate::hotkey;
use crate::mapping::{BlurStyle, TintSource};
use crate::package;
use crate::rules::{self, HotkeyAction};
use crate::system;
use crate::theme::{self, ACCENT, MUTED, TEXT, WARN};
use crate::tray;
use crate::update;
use egui::{Align, Color32, CornerRadius, Layout, RichText, Sense, Ui};

const RENAME: &str = "preset-rename";

impl BlurmanApp {
    pub(super) fn presets_page(&mut self, ui: &mut Ui) {
        let missing = rules::default_presets()
            .into_iter()
            .filter(|preset| self.settings.preset(&preset.name).is_none())
            .count();
        let mut restore = false;
        widgets::page_header(
            ui,
            "Presets",
            "Saved looks you can put on any app. Save new ones from an app's Look tab.",
            |ui| {
                if missing > 0
                    && ui
                        .button("Add the built-in presets back")
                        .on_hover_text("Subtle, Heavy glass, Vibrant, Smoke, and Milky.")
                        .clicked()
                {
                    restore = true;
                }
            },
        );
        if restore {
            for preset in rules::default_presets() {
                if self.settings.preset(&preset.name).is_none() {
                    self.settings.presets.push(preset);
                }
            }
            self.save_settings();
        }
        let selected = self
            .selected
            .clone()
            .filter(|process| self.store.rule(process).is_some());
        let mut apply = None;
        let mut rename_done = None;
        egui::ScrollArea::vertical()
            .id_salt("presets")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.settings.presets.is_empty() {
                    theme::card(ui, |ui| {
                        ui.label(theme::muted(
                            "No presets. Open an app on the Apps page and use Save as preset.",
                        ));
                    });
                }
                let columns = if ui.available_width() > 640.0 { 2 } else { 1 };
                let gap = 12.0;
                let card_w = (ui.available_width() - gap * (columns - 1) as f32) / columns as f32;
                let presets = self.settings.presets.clone();
                for row in presets.chunks(columns) {
                    // Top-aligned, so cards of different heights still start level.
                    ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        for preset in row {
                            ui.allocate_ui_with_layout(
                                egui::vec2(card_w, 0.0),
                                Layout::top_down(Align::LEFT),
                                |ui| {
                                    ui.set_width(card_w);
                                    theme::card(ui, |ui| {
                                        swatch(ui, &preset.look);
                                        ui.add_space(6.0);
                                        match &mut self.renaming {
                                            Some((old, new)) if *old == preset.name => {
                                                let field = ui.add(
                                                    egui::TextEdit::singleline(new)
                                                        .id(egui::Id::new(RENAME))
                                                        .desired_width(ui.available_width()),
                                                );
                                                if field.lost_focus() {
                                                    rename_done = Some((old.clone(), new.clone()));
                                                }
                                            }
                                            _ => {
                                                ui.label(
                                                    RichText::new(&preset.name)
                                                        .size(16.0)
                                                        .strong()
                                                        .color(TEXT),
                                                );
                                            }
                                        }
                                        ui.add(
                                            egui::Label::new(
                                                theme::muted(preset_summary(&preset.look)).small(),
                                            )
                                            .wrap(),
                                        );
                                        ui.add_space(6.0);
                                        ui.horizontal(|ui| {
                                            if let Some(process) = &selected {
                                                if ui.button(format!("Use on {process}")).clicked()
                                                {
                                                    apply = Some((process.clone(), preset.look));
                                                }
                                            }
                                            if ui.button("Rename").clicked() {
                                                self.renaming = Some((
                                                    preset.name.clone(),
                                                    preset.name.clone(),
                                                ));
                                                ui.memory_mut(|memory| {
                                                    memory.request_focus(egui::Id::new(RENAME))
                                                });
                                            }
                                            if ui.button("Delete").clicked() {
                                                self.confirm = Some(Confirm::DeletePreset(
                                                    preset.name.clone(),
                                                ));
                                            }
                                        });
                                    });
                                },
                            );
                        }
                    });
                    ui.add_space(gap);
                }
            });
        if let Some((old, new)) = rename_done {
            self.renaming = None;
            let new = new.trim().to_string();
            let taken = self
                .settings
                .presets
                .iter()
                .any(|preset| preset.name.eq_ignore_ascii_case(&new) && preset.name != old);
            if !new.is_empty() && !taken {
                if let Some(preset) = self.settings.presets.iter_mut().find(|p| p.name == old) {
                    preset.name = new;
                    self.save_settings();
                }
            } else if taken {
                self.notify("Another preset already has that name");
            }
        }
        if let Some((process, look)) = apply {
            self.select(process.clone());
            self.look = look;
            self.look_changed();
            self.notify(format!("{process} now uses that look"));
        }
    }

    pub(super) fn shortcuts_page(&mut self, ui: &mut Ui) {
        let mut changed = false;
        widgets::page_header(
            ui,
            "Shortcuts",
            "Keys that work from any app, even while Blurman is in the tray.",
            |ui| {
                changed |= widgets::switch(ui, &mut self.settings.hotkeys.enabled).changed();
                ui.label(theme::muted(if self.settings.hotkeys.enabled {
                    "On"
                } else {
                    "Off"
                }));
            },
        );
        let problems = self.shared.hotkey_problems.get();
        theme::card(ui, |ui| {
            ui.add_enabled_ui(self.settings.hotkeys.enabled, |ui| {
                for (index, action) in HotkeyAction::ALL.into_iter().enumerate() {
                    if index > 0 {
                        ui.separator();
                    }
                    let row_w = ui.available_width();
                    ui.horizontal(|ui| {
                        ui.set_width(row_w);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            ui.label(RichText::new(action.label()).color(TEXT));
                            ui.label(theme::muted(action_detail(action)).small());
                            if let Some((_, problem)) = problems.iter().find(|(a, _)| *a == action)
                            {
                                if self.settings.hotkeys.enabled {
                                    ui.label(RichText::new(problem).color(WARN).small());
                                }
                            }
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let current = self.settings.hotkeys.get(action).to_string();
                            if !current.is_empty()
                                && ui
                                    .small_button("Clear")
                                    .on_hover_text("Turn this shortcut off.")
                                    .clicked()
                            {
                                self.settings.hotkeys.set(action, String::new());
                                changed = true;
                            }
                            let recording = self.recording == Some(action);
                            let text = if recording {
                                "Press keys…".to_string()
                            } else if current.is_empty() {
                                "Not set".to_string()
                            } else {
                                current
                            };
                            let button = if recording {
                                egui::Button::new(RichText::new(text).color(Color32::WHITE))
                                    .fill(ACCENT)
                            } else {
                                egui::Button::new(RichText::new(text).monospace())
                            };
                            if ui
                                .add(button.min_size(egui::vec2(170.0, 30.0)))
                                .on_hover_text(
                                    "Click, then press the new keys. Esc keeps the old ones.",
                                )
                                .clicked()
                            {
                                self.recording = if recording { None } else { Some(action) };
                            }
                        });
                    });
                }
            });
        });
        ui.add_space(8.0);
        ui.label(
            theme::muted(
                "A shortcut needs Ctrl or Alt, so it never gets in the way of typing. F-keys work alone.",
            )
            .small(),
        );
        if changed {
            self.recording = None;
            self.save_settings();
        }
    }

    /// While a shortcut is recording, the next key press with Ctrl or Alt becomes the shortcut.
    /// The keys are taken out of the input so they do not also press a button.
    pub(super) fn record_shortcut(&mut self, ctx: &egui::Context) {
        let Some(action) = self.recording else {
            return;
        };
        let events = ctx.input_mut(|input| {
            let events = input.events.clone();
            input
                .events
                .retain(|event| !matches!(event, egui::Event::Key { .. } | egui::Event::Text(_)));
            events
        });
        for event in events {
            let egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = event
            else {
                continue;
            };
            let combo = match key {
                egui::Key::Escape => {
                    self.recording = None;
                    return;
                }
                egui::Key::Backspace | egui::Key::Delete if !modifiers.any() => String::new(),
                _ => match hotkey::from_egui(key, modifiers) {
                    Some(combo) => combo,
                    None => continue,
                },
            };
            let taken = HotkeyAction::ALL.into_iter().find(|other| {
                *other != action
                    && !combo.is_empty()
                    && self
                        .settings
                        .hotkeys
                        .get(*other)
                        .eq_ignore_ascii_case(&combo)
            });
            if let Some(other) = taken {
                self.notify(format!("{combo} is already used for {}", other.label()));
            } else {
                self.settings.hotkeys.set(action, combo);
                self.save_settings();
            }
            self.recording = None;
            return;
        }
    }

    pub(super) fn auto_pause_page(&mut self, ui: &mut Ui) {
        widgets::page_header(
            ui,
            "Auto-pause",
            "Put every app back on its own for a while. Your rules stay as they are.",
            |_| {},
        );
        let now = self.shared.auto_paused.get();
        theme::card(ui, |ui| {
            let (text, color) = match &now {
                Some(reason) => (format!("Auto-paused now: {reason}."), WARN),
                None => ("Not auto-paused right now.".to_string(), MUTED),
            };
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(egui::vec2(10.0, 18.0), Sense::hover());
                ui.painter().circle_filled(dot.center(), 4.0, color);
                ui.label(RichText::new(text).color(color));
            });
        });
        ui.add_space(10.0);
        let mut changed = false;
        theme::card(ui, |ui| {
            let auto = &mut self.settings.auto_pause;
            changed |= widgets::switch_row(
                ui,
                &mut auto.battery_saver,
                "While battery saver is on",
                "Saves the little power the glass uses.",
            );
            ui.separator();
            changed |= widgets::switch_row(
                ui,
                &mut auto.quiet,
                "During Do not disturb or presentation mode",
                "Includes the Do not disturb Windows turns on for games and full-screen video.",
            );
            ui.separator();
            ui.horizontal(|ui| {
                changed |= widgets::switch(ui, &mut auto.schedule).changed();
                ui.add_space(4.0);
                ui.label(RichText::new("Every day between").color(TEXT));
                changed |= widgets::clock_field(ui, "auto-from", &mut auto.from);
                ui.label(RichText::new("and").color(TEXT));
                changed |= widgets::clock_field(ui, "auto-until", &mut auto.until);
            });
            ui.label(
                theme::muted(
                    "Times are 24-hour. A span past midnight, like 22:00 to 07:00, works.",
                )
                .small(),
            );
        });
        if changed {
            self.save_settings();
        }
    }

    pub(super) fn settings_page(&mut self, ui: &mut Ui) {
        widgets::page_header(ui, "Settings", "Startup, updates, and your files.", |_| {});
        egui::ScrollArea::vertical()
            .id_salt("settings")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                widgets::section_label(ui, "Startup");
                theme::card(ui, |ui| {
                    let mut autostart = self.autostart;
                    if widgets::switch_row(
                        ui,
                        &mut autostart,
                        "Start when I sign in to Windows",
                        "Blurman starts quietly in the tray.",
                    ) {
                        match autostart::set_enabled(autostart) {
                            Ok(()) => self.autostart = autostart,
                            Err(err) => self.shared.set_status(err),
                        }
                    }
                    ui.separator();
                    if widgets::switch_row(
                        ui,
                        &mut self.settings.close_to_tray,
                        "Keep running in the tray when the window closes",
                        "Apps stay frosted. Click the tray icon to come back, or right-click it to pause, switch apps, or exit.",
                    ) {
                        self.save_settings();
                        self.tray_error = tray::sync(self.wants_tray(), &self.store).err();
                    }
                    if let Some(err) = &self.tray_error {
                        ui.colored_label(WARN, err);
                    }
                });

                widgets::section_label(ui, "Updates");
                if package::is_packaged() {
                    theme::card(ui, |ui| {
                        ui.label(RichText::new("Updates come from the Microsoft Store").color(TEXT));
                        ui.label(
                            theme::muted("The Store installs new versions of Blurman on its own.")
                                .small(),
                        );
                    });
                } else {
                theme::card(ui, |ui| {
                    if widgets::switch_row(
                        ui,
                        &mut self.settings.check_updates,
                        "Check for updates once a day",
                        "Off unless you turn it on. Asks GitHub for the latest release and shows a link. Nothing is downloaded, and nothing else is sent anywhere.",
                    ) {
                        if !self.settings.check_updates {
                            self.shared.set_update(None);
                        }
                        self.save_settings();
                    }
                    ui.add_space(4.0);
                    if ui.button("Check now").clicked() {
                        let shared = self.shared.clone();
                        std::thread::spawn(move || match update::check() {
                            Ok(Some(found)) => shared.set_update(Some(found)),
                            Ok(None) => shared.set_notice(format!(
                                "Blurman {} is the latest",
                                env!("CARGO_PKG_VERSION")
                            )),
                            Err(err) => shared.set_notice(format!("Could not check: {err}")),
                        });
                    }
                });
                }

                widgets::section_label(ui, "Files");
                theme::card(ui, |ui| {
                    ui.label(theme::muted("Rules, presets, and settings are saved in"));
                    let dir = rules::app_dir_on_disk();
                    ui.add(
                        egui::Label::new(
                            RichText::new(dir.display().to_string())
                                .monospace()
                                .color(TEXT),
                        )
                        .wrap(),
                    );
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui.button("Open folder").clicked() {
                            let _ = std::fs::create_dir_all(&dir);
                            system::open(&dir.display().to_string());
                        }
                        ui.label(
                            theme::muted("blurman export and blurman import copy them to another PC.")
                                .small(),
                        );
                    });
                });

                widgets::section_label(ui, "Start over");
                theme::card(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new("Delete every rule").color(TEXT));
                            ui.label(
                                theme::muted("Every app goes back to normal. Presets and settings stay.")
                                    .small(),
                            );
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .add_enabled(
                                    !self.store.rules.is_empty(),
                                    widgets::danger_button("Delete all rules…"),
                                )
                                .clicked()
                            {
                                self.confirm = Some(Confirm::RestoreAll);
                            }
                        });
                    });
                });

                widgets::section_label(ui, "About");
                theme::card(ui, |ui| {
                    ui.label(
                        RichText::new(format!("Blurman {}", env!("CARGO_PKG_VERSION")))
                            .strong()
                            .color(TEXT),
                    );
                    ui.label(theme::muted(
                        "Each frosted app is faded, and a blurred glass window sits right behind it.",
                    ));
                    ui.horizontal(|ui| {
                        if ui.link("github.com/xnixy/blurman").clicked() {
                            system::open("https://github.com/xnixy/blurman");
                        }
                        ui.label(theme::muted("· MIT license"));
                    });
                });
                ui.add_space(8.0);
            });
    }
}

fn action_detail(action: HotkeyAction) -> &'static str {
    match action {
        HotkeyAction::Pause => "Every app goes back to normal, or frosts again.",
        HotkeyAction::FrostFront => "Frosts the app you are using, or switches its rule off.",
        HotkeyAction::Peek => "Every frosted app shows solid while you hold the keys.",
        HotkeyAction::MoreGlass => "Makes the app you are using 5% more see-through.",
        HotkeyAction::LessGlass => "Makes the app you are using 5% less see-through.",
    }
}

/// A strip that hints at a preset: its tint, and stripes for frost or a solid wash for acrylic.
fn swatch(ui: &mut Ui, look: &crate::mapping::Look) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 36.0), Sense::hover());
    let painter = ui.painter();
    let base = Color32::from_rgb(170, 60, 80);
    let finish = &look.finish;
    let tint = match finish.tint_source {
        TintSource::Custom => Color32::from_rgb(finish.tint.0, finish.tint.1, finish.tint.2),
        TintSource::Theme => Color32::from_rgb(0x20, 0x20, 0x20),
        TintSource::Accent => Color32::from_rgb(0x00, 0x78, 0xd4),
    };
    let amount = match look.style {
        BlurStyle::Acrylic => 0.55,
        BlurStyle::Frost => finish.tint_amount as f32 / 100.0,
    };
    let saturated = {
        let grey = (base.r() as f32 + base.g() as f32 + base.b() as f32) / 3.0;
        let s = finish.saturation as f32 / 100.0;
        let mix = |c: u8| (grey + (c as f32 - grey) * s).clamp(0.0, 255.0) as u8;
        Color32::from_rgb(mix(base.r()), mix(base.g()), mix(base.b()))
    };
    let color = saturated.lerp_to_gamma(tint, amount);
    painter.rect_filled(rect, CornerRadius::same(8), color);
    // Lighter where more shows through.
    let glass = Color32::from_white_alpha((look.transparency as f32 * 1.4) as u8);
    let band =
        egui::Rect::from_min_max(rect.left_top(), egui::pos2(rect.center().x, rect.bottom()));
    painter.rect_filled(
        band,
        CornerRadius {
            nw: 8,
            sw: 8,
            ne: 0,
            se: 0,
        },
        glass.gamma_multiply(0.25),
    );
}
