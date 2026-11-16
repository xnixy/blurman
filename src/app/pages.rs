//! Presets, Shortcuts, Auto-pause, and Settings.

use super::apps::preset_summary;
use super::widgets::{self, Icon};
use super::{BlurmanApp, Confirm, GAP};
use crate::autostart;
use crate::hotkey;
use crate::mapping::{BlurStyle, TintSource};
use crate::package;
use crate::rules::{self, HotkeyAction};
use crate::system;
use crate::theme::{self, colors, Mode};
use crate::tray;
use crate::update;
use egui::{Align, Color32, CornerRadius, Layout, RichText, Sense, Ui};

const RENAME: &str = "preset-rename";
/// The narrowest a preset's island gets before the grid drops a column.
const PRESET_WIDTH: f32 = 250.0;

impl BlurmanApp {
    /// Saved looks, as a grid of islands.
    pub(super) fn presets_page(&mut self, ui: &mut Ui) {
        let selected = self
            .selected
            .clone()
            .filter(|process| self.store.rule(process).is_some());
        let mut apply = None;
        let mut rename_done = None;
        egui::ScrollArea::vertical()
            .id_salt("presets")
            .auto_shrink([false, false])
            .drag_to_scroll(false)
            .show(ui, |ui| {
                if self.settings.presets.is_empty() {
                    widgets::card(ui, "", |ui| {
                        ui.add_space(20.0);
                        widgets::empty(
                            ui,
                            Icon::PRESETS,
                            "No presets. Open an app on the Apps page and use Save as preset.",
                        );
                        ui.add_space(20.0);
                    });
                }
                let room = ui.available_width();
                let columns = (((room + GAP) / (PRESET_WIDTH + GAP)).floor() as usize).max(1);
                let card_w = ((room - GAP * (columns - 1) as f32) / columns as f32).floor();
                // The last card in a full row takes what rounding left over, so the grid ends
                // where the header does.
                let last_w = room - (card_w + GAP) * (columns - 1) as f32;
                let presets = self.settings.presets.clone();
                for (index, row) in presets.chunks(columns).enumerate() {
                    // Every card in a row is as tall as its tallest was on the last frame.
                    let height_id = egui::Id::new(("preset row height", index));
                    let row_h = ui
                        .data(|data| data.get_temp::<f32>(height_id))
                        .unwrap_or(0.0);
                    let mut tallest = 0.0_f32;
                    ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.x = GAP;
                        for (column, preset) in row.iter().enumerate() {
                            let card_w = if column + 1 == columns {
                                last_w
                            } else {
                                card_w
                            };
                            ui.allocate_ui_with_layout(
                                egui::vec2(card_w, 0.0),
                                Layout::top_down(Align::LEFT),
                                |ui| {
                                    ui.set_width(card_w);
                                    let ((), own) = widgets::card_at_least(ui, "", row_h, |ui| {
                                        look_strip(ui, &preset.look);
                                        ui.add_space(2.0);
                                        match &mut self.renaming {
                                            Some((old, new)) if *old == preset.name => {
                                                let field = ui.add(
                                                    egui::TextEdit::singleline(new)
                                                        .id(egui::Id::new(RENAME))
                                                        .margin(widgets::FIELD_MARGIN)
                                                        .desired_width(ui.available_width() - 24.0),
                                                );
                                                widgets::name(ui, &field, "Preset name");
                                                if field.lost_focus() {
                                                    // Esc gives the new name up. Enter, or a
                                                    // click elsewhere, keeps it.
                                                    let kept = !ui.input(|input| {
                                                        input.key_pressed(egui::Key::Escape)
                                                    });
                                                    rename_done = Some((
                                                        old.clone(),
                                                        kept.then(|| new.clone()),
                                                    ));
                                                }
                                            }
                                            _ => {
                                                ui.add(
                                                    egui::Label::new(
                                                        RichText::new(&preset.name)
                                                            .font(theme::semibold(14.0))
                                                            .color(colors().text),
                                                    )
                                                    .truncate(),
                                                );
                                            }
                                        }
                                        ui.add(
                                            egui::Label::new(theme::detail(preset_summary(
                                                &preset.look,
                                            )))
                                            .wrap(),
                                        );
                                        ui.add_space(2.0);
                                        ui.horizontal_wrapped(|ui| {
                                            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                                            if let Some(process) = &selected {
                                                if widgets::secondary(
                                                    ui,
                                                    &format!("Use on {process}"),
                                                )
                                                .clicked()
                                                {
                                                    apply = Some((process.clone(), preset.look));
                                                }
                                            }
                                            if widgets::secondary(ui, "Rename").clicked() {
                                                self.renaming = Some((
                                                    preset.name.clone(),
                                                    preset.name.clone(),
                                                ));
                                                ui.memory_mut(|memory| {
                                                    memory.request_focus(egui::Id::new(RENAME))
                                                });
                                            }
                                            if widgets::secondary(ui, "Delete…").clicked() {
                                                self.confirm = Some(Confirm::DeletePreset(
                                                    preset.name.clone(),
                                                ));
                                            }
                                        });
                                    });
                                    tallest = tallest.max(own);
                                },
                            );
                        }
                    });
                    ui.data_mut(|data| data.insert_temp(height_id, tallest));
                    // The row and the island each leave a gap: together, the 12 between islands.
                    ui.add_space(-ui.spacing().item_spacing.y);
                }
            });
        if let Some((old, new)) = rename_done {
            self.renaming = None;
            match new.map(|new| self.settings.rename_preset(&old, &new)) {
                Some(rules::Renamed::Done) => self.save_settings(),
                Some(rules::Renamed::Taken) => self.notify("Another preset already has that name"),
                Some(rules::Renamed::Kept) | None => {}
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
        widgets::card(ui, "", |ui| {
            changed |= widgets::switch_row(
                ui,
                &mut self.settings.hotkeys.enabled,
                "Use shortcuts",
                "Keys that work from any app, even while Blurman is in the tray.",
            );
        });
        let problems = self.shared.hotkey_problems.get();
        let enabled = self.settings.hotkeys.enabled;
        widgets::card(ui, "Shortcuts", |ui| {
            ui.add_enabled_ui(enabled, |ui| {
                for (index, action) in HotkeyAction::ALL.into_iter().enumerate() {
                    if index > 0 {
                        widgets::divider(ui);
                    }
                    let problem = problems
                        .iter()
                        .find(|(known, _)| *known == action)
                        .map(|(_, problem)| problem.clone())
                        .filter(|_| enabled);
                    widgets::trailing(
                        ui,
                        |ui| {
                            let current = self.settings.hotkeys.get(action).to_string();
                            if !current.is_empty()
                                && widgets::secondary(ui, "Clear")
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
                            if widgets::sized(ui, &text, recording, 140.0)
                                .on_hover_text(
                                    "Click, then press the new keys. Esc keeps the old ones.",
                                )
                                .clicked()
                            {
                                self.recording = if recording { None } else { Some(action) };
                            }
                        },
                        |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(action.label()).color(colors().text),
                                )
                                .wrap(),
                            );
                            ui.add(egui::Label::new(theme::detail(action_detail(action))).wrap());
                            if let Some(problem) = &problem {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(problem).size(11.5).color(colors().warn),
                                    )
                                    .wrap(),
                                );
                            }
                        },
                    );
                }
            });
        });
        widgets::card(ui, "", |ui| {
            ui.add(
                egui::Label::new(theme::muted(
                    "A shortcut needs Ctrl or Alt, so it never gets in the way of typing. F-keys work alone.",
                ))
                .wrap(),
            );
        });
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
        // Keys pressed in another app are not for this. Leaving the window ends the recording,
        // and the shortcuts work again.
        if !ctx.input(|input| input.focused) {
            self.recording = None;
            return;
        }
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
                _ => match hotkey::from_press(key, modifiers) {
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
        let now = self.shared.auto_paused.get();
        widgets::card(ui, "Now", |ui| {
            let c = colors();
            let (text, color) = match &now {
                Some(reason) => (format!("Auto-paused: {reason}."), c.warn),
                None => ("Not auto-paused right now.".to_string(), c.text),
            };
            ui.add(egui::Label::new(RichText::new(text).color(color)).wrap());
            ui.add(
                egui::Label::new(theme::detail(
                    "While auto-paused, every app is back to normal. Your rules stay as they are.",
                ))
                .wrap(),
            );
        });
        let mut changed = false;
        let mut times_done = false;
        widgets::card(ui, "Pause on its own", |ui| {
            let auto = &mut self.settings.auto_pause;
            changed |= widgets::switch_row(
                ui,
                &mut auto.battery_saver,
                "While battery saver is on",
                "Saves the little power the glass uses.",
            );
            widgets::divider(ui);
            changed |= widgets::switch_row(
                ui,
                &mut auto.quiet,
                "During Do not disturb or presentation mode",
                "Includes the Do not disturb Windows turns on for games and full-screen video.",
            );
            widgets::divider(ui);
            widgets::trailing(
                ui,
                |ui| changed |= widgets::switch(ui, &mut auto.schedule, "Every day").changed(),
                |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("Every day between").color(colors().text));
                        times_done |= widgets::clock_field(ui, "auto-from", "From", &mut auto.from);
                        ui.label(RichText::new("and").color(colors().text));
                        times_done |=
                            widgets::clock_field(ui, "auto-until", "Until", &mut auto.until);
                    });
                    ui.add(
                        egui::Label::new(theme::detail(
                            "Times are 24-hour. A span past midnight, like 22:00 to 07:00, works.",
                        ))
                        .wrap(),
                    );
                },
            );
        });
        if times_done {
            // A time that does not read as one goes back to the last that did.
            let last = self.settings_on_disk.auto_pause.clone();
            self.settings.auto_pause.keep_times_that_read(&last);
            changed |= self.settings != self.settings_on_disk;
        }
        if changed {
            self.save_settings();
        }
    }

    pub(super) fn settings_page(&mut self, ui: &mut Ui) {
        self.appearance_card(ui);
        widgets::card(ui, "Startup", |ui| {
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
            widgets::divider(ui);
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
                ui.add(egui::Label::new(RichText::new(err).color(colors().warn)).wrap());
            }
        });

        if package::is_packaged() {
            widgets::card(ui, "Updates", |ui| {
                ui.label(
                    RichText::new("Updates come from the Microsoft Store").color(colors().text),
                );
                ui.add(
                    egui::Label::new(theme::detail(
                        "The Store installs new versions of Blurman on its own.",
                    ))
                    .wrap(),
                );
            });
        } else {
            widgets::card(ui, "Updates", |ui| {
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
                    // The daily check reads the setting again now, not up to a day later.
                    self.shared.wake_update_watcher();
                }
                widgets::divider(ui);
                let check = widgets::setting_row(
                    ui,
                    "Get Blurman from the Microsoft Store",
                    "Signed by Microsoft, and it updates itself.",
                    |ui| {
                        let check = widgets::secondary(ui, "Check now").clicked();
                        if widgets::secondary(ui, "Open the Store").clicked() {
                            system::open(package::STORE_URL);
                        }
                        check
                    },
                );
                if check {
                    let shared = self.shared.clone();
                    std::thread::spawn(move || match update::check_once(&shared) {
                        Some(Ok(Some(found))) => shared.set_update(Some(found)),
                        Some(Ok(None)) => shared.set_notice(format!(
                            "Blurman {} is the latest",
                            env!("CARGO_PKG_VERSION")
                        )),
                        Some(Err(err)) => shared.set_notice(format!("Could not check: {err}")),
                        None => shared.set_notice("Already checking"),
                    });
                }
            });
        }

        widgets::card(ui, "Files", |ui| {
            let dir = rules::app_dir_on_disk();
            let open = widgets::trailing(
                ui,
                |ui| widgets::secondary(ui, "Open folder").clicked(),
                |ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new("Rules, presets, and settings are saved in")
                                .color(colors().text),
                        )
                        .wrap(),
                    );
                    ui.add(
                        egui::Label::new(
                            RichText::new(dir.display().to_string())
                                .monospace()
                                .color(colors().muted),
                        )
                        .wrap(),
                    );
                    ui.add(
                        egui::Label::new(theme::detail(
                            "blurman export and blurman import copy them to another PC.",
                        ))
                        .wrap(),
                    );
                },
            );
            if open {
                let _ = std::fs::create_dir_all(&dir);
                system::open(&dir.display().to_string());
            }
        });

        widgets::card(ui, "Start over", |ui| {
            let any = !self.store.rules.is_empty();
            let delete = widgets::setting_row(
                ui,
                "Delete every rule",
                "Every app goes back to normal. Presets and settings stay.",
                |ui| {
                    ui.add_enabled_ui(any, |ui| widgets::danger(ui, "Delete all rules…").clicked())
                        .inner
                },
            );
            if delete {
                self.confirm = Some(Confirm::RestoreAll);
            }
        });

        widgets::card(ui, "About", |ui| {
            let open = widgets::trailing(
                ui,
                |ui| widgets::secondary(ui, "Open GitHub").clicked(),
                |ui| {
                    ui.label(
                        RichText::new(format!("Blurman {}", env!("CARGO_PKG_VERSION")))
                            .font(theme::semibold(13.0))
                            .color(colors().text),
                    );
                    ui.add(
                        egui::Label::new(theme::detail(
                            "Each frosted app is faded, and a blurred glass window sits right behind it. MIT license.",
                        ))
                        .wrap(),
                    );
                },
            );
            if open {
                system::open("https://github.com/xnixy/blurman");
            }
        });
    }

    /// Accent, theme, window style, and the sidebar.
    fn appearance_card(&mut self, ui: &mut Ui) {
        let mut changed = false;
        widgets::card(ui, "Appearance", |ui| {
            let look = &mut self.settings.appearance;
            let light = colors().light;
            ui.label(RichText::new("Accent color").color(colors().text));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                let named = theme::ACCENTS
                    .iter()
                    .any(|(name, ..)| name.eq_ignore_ascii_case(&look.accent));
                if widgets::chip(ui, !named, "Windows")
                    .on_hover_text("The accent color chosen in Windows.")
                    .clicked()
                    && named
                {
                    look.accent = "Windows".into();
                    changed = true;
                }
                for (name, ..) in theme::ACCENTS {
                    let selected = name.eq_ignore_ascii_case(&look.accent);
                    let color = theme::accent_for(name, light);
                    if widgets::swatch(ui, color, selected, name).clicked() && !selected {
                        look.accent = name.into();
                        changed = true;
                    }
                }
            });
            widgets::divider(ui);

            ui.label(RichText::new("Theme").color(colors().text));
            let mut theme_now = match look.theme.as_str() {
                "light" => "light",
                "dark" => "dark",
                _ => "windows",
            };
            if widgets::segmented(
                ui,
                &mut theme_now,
                &[
                    ("windows", "Windows", "Light or dark, as Windows is."),
                    ("light", "Light", ""),
                    ("dark", "Dark", ""),
                ],
            ) {
                look.theme = theme_now.into();
                changed = true;
            }
            widgets::divider(ui);

            ui.label(RichText::new("Window").color(colors().text));
            let mut style = match look.window.as_str() {
                "frosted" => "frosted",
                "solid" => "solid",
                _ => "glass",
            };
            let forced = theme::high_contrast() || theme::forced_solid();
            ui.add_enabled_ui(!forced, |ui| {
                if widgets::segmented(
                    ui,
                    &mut style,
                    &[
                        ("glass", "Invisible", ""),
                        ("frosted", "Frosted", ""),
                        ("solid", "No transparency", ""),
                    ],
                ) {
                    look.window = style.into();
                    changed = true;
                }
            });
            let about = if theme::high_contrast() {
                "Windows' high contrast theme is on, so the window is solid."
            } else if theme::transparency_off() {
                "Transparency effects are off in Windows, so the window is solid."
            } else if forced {
                "Over Remote Desktop the window is solid."
            } else if style == "glass" && self.backdrop.mode != Mode::Glass {
                "This PC cannot draw the floating glass, so the window is frosted instead."
            } else {
                match style {
                    "frosted" => "One frosted window behind all the islands.",
                    "solid" => "A solid window, with nothing showing through.",
                    _ => "Only the islands show, with the desktop between them.",
                }
            };
            ui.add(egui::Label::new(theme::detail(about)).wrap());
            widgets::divider(ui);

            changed |= widgets::switch_row(
                ui,
                &mut look.compact_sidebar,
                "Compact sidebar",
                "Show only the icons. Page names appear when you point at them.",
            );
        });
        if changed {
            self.save_settings();
            // A new window style makes the window again, in the same place.
            let ctx = ui.ctx().clone();
            self.remake_if_needed(&ctx);
        }
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

/// A strip that hints at a preset: its tint, and a lighter half where more shows through.
fn look_strip(ui: &mut Ui, look: &crate::mapping::Look) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 36.0), Sense::hover());
    let painter = ui.painter();
    let base = Color32::from_rgb(170, 60, 80);
    let finish = &look.finish;
    // Theme and accent tints show what they are on this PC right now.
    let windows = system::system_colors();
    let tint = match finish.tint_source {
        TintSource::Custom => finish.tint,
        TintSource::Theme => windows.theme_tint(),
        TintSource::Accent => windows.accent,
    };
    let tint = Color32::from_rgb(tint.0, tint.1, tint.2);
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
    let color = saturated
        .lerp_to_gamma(Color32::from_gray(0xf2), finish.milk as f32 / 100.0 * 0.6)
        .lerp_to_gamma(tint, amount);
    painter.rect_filled(rect, CornerRadius::same(12), color);
    // Lighter where more shows through.
    let glass = Color32::from_white_alpha((look.transparency as f32 * 1.4) as u8);
    let band =
        egui::Rect::from_min_max(rect.left_top(), egui::pos2(rect.center().x, rect.bottom()));
    painter.rect_filled(
        band,
        CornerRadius {
            nw: 12,
            sw: 12,
            ne: 0,
            se: 0,
        },
        glass.gamma_multiply(0.25),
    );
    if colors().high_contrast {
        painter.rect_stroke(
            rect,
            CornerRadius::same(12),
            egui::Stroke::new(1.0_f32, colors().text),
            egui::StrokeKind::Inside,
        );
    }
}
