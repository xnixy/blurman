//! The Apps page: every app in one list, and an editor for the one you pick.

use super::widgets::{self, RowInfo};
use super::{BlurmanApp, Confirm, EditorTab};
use crate::mapping::{self, BlurStyle, FocusLook, Look, Rgb, TintSource};
use crate::picker;
use crate::rules::Preset;
use crate::theme::{self, TEXT, WARN};
use egui::{Align, Frame, Layout, Margin, RichText, Slider, Ui};
use std::sync::atomic::Ordering;

const PRESET_NAME: &str = "preset-name";

/// One app in the list, from its rule, its open windows, or both.
struct Entry {
    process: String,
    subtitle: String,
    path: Option<String>,
    /// Some(on) when the app has a rule.
    enabled: Option<bool>,
    warning: Option<&'static str>,
}

impl BlurmanApp {
    pub(super) fn apps_page(&mut self, ui: &mut Ui) {
        let mut pick = false;
        widgets::page_header(
            ui,
            "Apps",
            "Choose an app, give it a look, and frost it.",
            |ui| {
                pick = ui
                    .add(theme::primary_button("Pick a window").min_size(egui::vec2(130.0, 32.0)))
                    .on_hover_text(
                        "Blurman steps aside. Click any window to choose its app, or press Esc.",
                    )
                    .clicked();
            },
        );
        if pick {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            picker::start(self.shared.clone());
        }
        let list_w = (ui.available_width() * 0.42).clamp(270.0, 360.0);
        egui::SidePanel::left("app-list")
            .resizable(false)
            .exact_width(list_w)
            .show_separator_line(false)
            .frame(Frame::NONE)
            .show_inside(ui, |ui| self.app_list(ui));
        egui::CentralPanel::default()
            .frame(Frame::NONE.inner_margin(Margin {
                left: 14,
                ..Margin::ZERO
            }))
            .show_inside(ui, |ui| self.editor(ui));
    }

    /// Frosted apps first, then apps whose rule is off, then everything else that is open.
    fn entries(&self) -> [Vec<Entry>; 3] {
        let needle = self.search.trim().to_lowercase();
        let matches = |process: &str, title: &str| {
            needle.is_empty()
                || process.to_lowercase().contains(&needle)
                || title.to_lowercase().contains(&needle)
        };
        let mut lists: [Vec<Entry>; 3] = Default::default();
        for rule in &self.store.rules {
            let group = self
                .groups
                .iter()
                .find(|group| group.process.eq_ignore_ascii_case(&rule.process));
            let title = group.map(|group| group.sample_title.as_str()).unwrap_or("");
            if !matches(&rule.process, title) {
                continue;
            }
            let subtitle = match group {
                Some(group) if group.windows > 1 => {
                    format!("{} windows · {}", group.windows, group.sample_title)
                }
                Some(group) => group.sample_title.clone(),
                None => "Not running".to_string(),
            };
            let list = if rule.enabled { 0 } else { 1 };
            lists[list].push(Entry {
                process: rule.process.clone(),
                subtitle,
                path: group.and_then(|group| group.path.clone()),
                enabled: Some(rule.enabled),
                warning: group.and_then(warning_for),
            });
        }
        for group in &self.groups {
            if self.store.rule(&group.process).is_some()
                || !matches(&group.process, &group.sample_title)
            {
                continue;
            }
            lists[2].push(Entry {
                process: group.process.clone(),
                subtitle: group.sample_title.clone(),
                path: group.path.clone(),
                enabled: None,
                warning: warning_for(group),
            });
        }
        lists
    }

    fn app_list(&mut self, ui: &mut Ui) {
        theme::card(ui, |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Search apps")
                    .desired_width(ui.available_width()),
            );
            ui.add_space(4.0);
            let lists = self.entries();
            let mut picked = None;
            let mut toggled = None;
            egui::ScrollArea::vertical()
                .id_salt("apps")
                .auto_shrink([false, false])
                .max_height(ui.available_height())
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    let titles = ["Frosted", "Off", "Open apps"];
                    for (list, title) in lists.iter().zip(titles) {
                        if list.is_empty() {
                            continue;
                        }
                        widgets::section_label(ui, title);
                        for entry in list {
                            let icon =
                                self.icons
                                    .get(ui.ctx(), &entry.process, entry.path.as_deref());
                            let mut enabled = entry.enabled;
                            let response = widgets::app_row(
                                ui,
                                &mut RowInfo {
                                    process: &entry.process,
                                    subtitle: &entry.subtitle,
                                    icon: icon.as_ref(),
                                    selected: self.selected.as_deref()
                                        == Some(entry.process.as_str()),
                                    warning: entry.warning,
                                    enabled: enabled.as_mut(),
                                },
                            );
                            if response.toggled {
                                toggled = Some((entry.process.clone(), enabled == Some(true)));
                            } else if response.clicked {
                                picked = Some(entry.process.clone());
                            }
                        }
                    }
                    if lists.iter().all(Vec::is_empty) {
                        ui.add_space(12.0);
                        let text = if self.search.trim().is_empty() {
                            "No app windows are open."
                        } else {
                            "No app matches that search."
                        };
                        ui.label(theme::muted(text));
                    }
                });
            if let Some((process, on)) = toggled {
                self.set_enabled(&process, on);
            }
            if let Some(process) = picked {
                self.select(process);
            }
        });
    }

    fn editor(&mut self, ui: &mut Ui) {
        let Some(process) = self.selected.clone() else {
            empty_editor(ui);
            return;
        };
        let group = self
            .groups
            .iter()
            .find(|group| group.process.eq_ignore_ascii_case(&process))
            .cloned();
        let ruled = self.selected_rule().map(|rule| rule.enabled);
        egui::ScrollArea::vertical()
            .id_salt("editor")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                theme::card(ui, |ui| {
                    ui.horizontal(|ui| {
                        let icon = self.icons.get(
                            ui.ctx(),
                            &process,
                            group.as_ref().and_then(|group| group.path.as_deref()),
                        );
                        widgets::app_icon(ui, icon.as_ref(), &process, 44.0);
                        ui.add_space(4.0);
                        let side_w = 150.0;
                        let text_w = (ui.available_width() - side_w).max(60.0);
                        ui.allocate_ui_with_layout(
                            egui::vec2(text_w, 44.0),
                            Layout::top_down(Align::LEFT),
                            |ui| {
                                ui.set_width(text_w);
                                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.label(RichText::new(&process).size(18.0).strong().color(TEXT));
                                let subtitle = group
                                    .as_ref()
                                    .map(|group| group.sample_title.clone())
                                    .unwrap_or_else(|| "Not running".into());
                                ui.label(theme::muted(subtitle).small());
                            },
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| match ruled {
                            Some(mut on) => {
                                if widgets::switch(ui, &mut on).changed() {
                                    self.set_enabled(&process, on);
                                }
                                ui.label(theme::muted(if on { "Frosted" } else { "Off" }));
                            }
                            None => {
                                if ui
                                    .add(
                                        theme::primary_button("Frost it")
                                            .min_size(egui::vec2(110.0, 32.0)),
                                    )
                                    .clicked()
                                {
                                    self.frost_selected();
                                }
                            }
                        });
                    });
                    if let Some(group) = &group {
                        if group.elevated {
                            note(ui, "This app runs as administrator. Run Blurman as administrator to frost it.");
                        } else if group.fullscreen {
                            note(ui, "This app covers the screen, so Blurman leaves it alone until it is windowed.");
                        }
                    } else if ruled.is_some() {
                        ui.add_space(4.0);
                        ui.label(
                            theme::muted("Not open now. Its windows are frosted when it starts.")
                                .small(),
                        );
                    }
                });
                ui.add_space(10.0);
                widgets::segmented(
                    ui,
                    &mut self.editor_tab,
                    &[
                        (EditorTab::Look, "Look", "Style, transparency, blur, and presets."),
                        (EditorTab::Finish, "Finish", "Tint, saturation, and grain."),
                        (
                            EditorTab::Behavior,
                            "Behavior",
                            "How it looks while you use it, and which windows.",
                        ),
                    ],
                );
                ui.add_space(8.0);
                theme::card(ui, |ui| match self.editor_tab {
                    EditorTab::Look => {
                        if self.look_tab(ui) {
                            self.look_changed();
                        }
                    }
                    EditorTab::Finish => {
                        let fallback = self.shared.fallback.load(Ordering::SeqCst);
                        let milky = self.look.style == BlurStyle::Acrylic || fallback;
                        if finish_controls(ui, &mut self.look, milky) {
                            self.look_changed();
                        }
                    }
                    EditorTab::Behavior => {
                        if self.behavior_tab(ui) {
                            self.behavior_to_rule();
                        }
                    }
                });
                if ruled.is_some() {
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add(widgets::danger_button("Remove rule…"))
                            .on_hover_text("Forget this app's look and behavior.")
                            .clicked()
                        {
                            self.confirm = Some(Confirm::RemoveRule(process.clone()));
                        }
                        ui.label(theme::muted("Changes apply as you make them.").small());
                    });
                }
            });
    }

    /// Style, transparency, blur, and presets. Returns whether the look changed.
    fn look_tab(&mut self, ui: &mut Ui) -> bool {
        let fallback = self.shared.fallback.load(Ordering::SeqCst);
        let mut moved = false;
        ui.horizontal(|ui| {
            ui.label(theme::muted("Style"));
            ui.add_space(8.0);
            moved |= widgets::segmented(
                ui,
                &mut self.look.style,
                &[
                    (
                        BlurStyle::Frost,
                        "Frost",
                        "Blur whatever is behind the window. The slider sets how far it spreads.",
                    ),
                    (
                        BlurStyle::Acrylic,
                        "Acrylic",
                        "Milky system glass. The slider sets how milky it is.",
                    ),
                ],
            );
        });
        if !fallback {
            ui.label(
                theme::muted(match self.look.style {
                    BlurStyle::Frost => "Blurs whatever is behind the window.",
                    BlurStyle::Acrylic => "Milky system glass. The slider sets how milky it is.",
                })
                .small(),
            );
        }
        ui.add_space(6.0);
        let milky = self.look.style == BlurStyle::Acrylic || fallback;
        moved |= widgets::slider_row(ui, "Transparency", |ui| {
            ui.add(
                Slider::new(
                    &mut self.look.transparency,
                    mapping::TRANSPARENCY_MIN..=mapping::TRANSPARENCY_MAX,
                )
                .suffix("%"),
            )
        });
        moved |= widgets::slider_row(ui, if milky { "Milkiness" } else { "Blur" }, |ui| {
            ui.add(Slider::new(
                &mut self.look.blur,
                mapping::BLUR_MIN..=mapping::BLUR_MAX,
            ))
        });
        ui.add_space(6.0);
        widgets::section_label(ui, "Presets");
        moved | self.presets_row(ui)
    }

    /// One click puts a saved look on the sliders.
    fn presets_row(&mut self, ui: &mut Ui) -> bool {
        let mut picked = None;
        let mut save = false;
        let mut cancel = false;
        ui.horizontal_wrapped(|ui| {
            for preset in &self.settings.presets {
                if theme::choice(ui, preset.look == self.look, &preset.name)
                    .on_hover_text(preset_summary(&preset.look))
                    .clicked()
                {
                    picked = Some(preset.look);
                }
            }
            match &mut self.naming_preset {
                None => {
                    if ui
                        .button("Save as preset…")
                        .on_hover_text("Keep this look to use on other apps.")
                        .clicked()
                    {
                        self.naming_preset = Some(String::new());
                        ui.memory_mut(|memory| memory.request_focus(egui::Id::new(PRESET_NAME)));
                    }
                }
                Some(name) => {
                    let field = ui.add(
                        egui::TextEdit::singleline(name)
                            .id(egui::Id::new(PRESET_NAME))
                            .hint_text("Preset name")
                            .desired_width(120.0),
                    );
                    let entered =
                        field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if ui.button("Save").clicked() || entered {
                        save = true;
                    }
                    if ui.button("Cancel").clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        cancel = true;
                    }
                }
            }
        });
        if cancel {
            self.naming_preset = None;
        }
        if save {
            let name = self.naming_preset.take().unwrap_or_default();
            let name = name.trim();
            if !name.is_empty() {
                self.settings.save_preset(Preset {
                    name: name.to_string(),
                    look: self.look.clamped(),
                });
                self.save_settings();
                self.notify(format!("Saved the {name} preset"));
            }
        }
        match picked {
            Some(look) if look != self.look => {
                self.look = look;
                true
            }
            _ => false,
        }
    }

    /// Focus look and title filters. Returns whether any of them changed and should be saved.
    fn behavior_tab(&mut self, ui: &mut Ui) -> bool {
        let mut changed = false;
        ui.label(RichText::new("While you use it").color(TEXT));
        ui.add_space(2.0);
        changed |= widgets::segmented(
            ui,
            &mut self.focus,
            &[
                (FocusLook::Same, "Same", ""),
                (FocusLook::Lighter, "Lighter", ""),
                (FocusLook::Solid, "Solid", ""),
            ],
        );
        ui.label(
            theme::muted(match self.focus {
                FocusLook::Same => "Stays frosted while it has focus.",
                FocusLook::Lighter => "Half as see-through while it has focus.",
                FocusLook::Solid => {
                    "Fully solid while it has focus, so text is easy to read. Frosted again when you move on."
                }
            })
            .small(),
        );
        ui.add_space(10.0);
        ui.label(RichText::new("Which windows").color(TEXT));
        ui.add_space(2.0);
        changed |= widgets::text_row(
            ui,
            "Only titles",
            &mut self.only_titles,
            "Frost only windows with these in the title",
        );
        changed |= widgets::text_row(
            ui,
            "Skip titles",
            &mut self.skip_titles,
            "Leave windows with these in the title alone",
        );
        ui.label(
            theme::muted(
                "Separate titles with |. Use * as a wildcard, like *- YouTube*. Leave both empty to frost every window.",
            )
            .small(),
        );
        changed
    }
}

fn warning_for(group: &crate::target::AppGroup) -> Option<&'static str> {
    if group.elevated {
        Some("Admin")
    } else if group.fullscreen {
        Some("Fullscreen")
    } else {
        None
    }
}

fn note(ui: &mut Ui, text: &str) {
    ui.add_space(6.0);
    ui.add(egui::Label::new(RichText::new(text).color(WARN).small()).wrap());
}

/// What to show before an app is picked.
fn empty_editor(ui: &mut Ui) {
    theme::card(ui, |ui| {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("No app selected").size(18.0).color(TEXT));
            ui.add_space(4.0);
            ui.add(
                egui::Label::new(theme::muted(
                    "Choose an app from the list, or use Pick a window to click the one you mean.",
                ))
                .wrap(),
            );
        });
        ui.add_space(24.0);
    });
}

/// "Frost · 40% · blur 60 · saturation 170%", for a preset's tooltip and card.
pub(super) fn preset_summary(look: &Look) -> String {
    let mut parts = vec![
        match look.style {
            BlurStyle::Frost => "Frost".to_string(),
            BlurStyle::Acrylic => "Acrylic".to_string(),
        },
        format!("{}% see-through", look.transparency),
        match look.style {
            BlurStyle::Frost => format!("blur {}", look.blur),
            BlurStyle::Acrylic => format!("milkiness {}", look.blur),
        },
    ];
    let finish = &look.finish;
    if finish.tint_source != TintSource::Custom {
        parts.push(match finish.tint_source {
            TintSource::Theme => "theme tint".to_string(),
            _ => "accent tint".to_string(),
        });
    } else if finish.tint_amount > 0 {
        parts.push(format!("tint {}%", finish.tint_amount));
    }
    if finish.saturation != mapping::SATURATION_DEFAULT {
        parts.push(format!("saturation {}%", finish.saturation));
    }
    if finish.grain > 0 {
        parts.push(format!("grain {}", finish.grain));
    }
    parts.join(" · ")
}

/// Tint, saturation, and grain. Acrylic only takes the tint color; the rest needs frost.
pub(super) fn finish_controls(ui: &mut Ui, look: &mut Look, milky: bool) -> bool {
    let mut changed = false;
    let finish = &mut look.finish;
    ui.horizontal_wrapped(|ui| {
        ui.label(theme::muted("Tint"));
        ui.add_space(8.0);
        changed |= widgets::segmented(
            ui,
            &mut finish.tint_source,
            &[
                (TintSource::Custom, "Color", "A color you pick."),
                (
                    TintSource::Theme,
                    "Theme",
                    "Dark or light, following the Windows app mode.",
                ),
                (
                    TintSource::Accent,
                    "Accent",
                    "The Windows accent color, following it as it changes.",
                ),
            ],
        );
        if finish.tint_source == TintSource::Custom {
            let mut rgb = [finish.tint.0, finish.tint.1, finish.tint.2];
            if egui::color_picker::color_edit_button_srgb(ui, &mut rgb).changed() {
                finish.tint = Rgb(rgb[0], rgb[1], rgb[2]);
                changed = true;
            }
        }
    });
    ui.add_space(6.0);
    if milky {
        ui.add(
            egui::Label::new(
                theme::muted(
                    "Acrylic is colored with the tint, and Milkiness sets how much. Saturation and grain need Frost.",
                )
                .small(),
            )
            .wrap(),
        );
        return changed;
    }
    changed |= widgets::slider_row(ui, "Tint amount", |ui| {
        ui.add(Slider::new(&mut finish.tint_amount, 0..=mapping::TINT_MAX).suffix("%"))
    });
    changed |= widgets::slider_row(ui, "Saturation", |ui| {
        ui.add(
            Slider::new(
                &mut finish.saturation,
                mapping::SATURATION_MIN..=mapping::SATURATION_MAX,
            )
            .suffix("%"),
        )
    });
    changed |= widgets::slider_row(ui, "Grain", |ui| {
        ui.add(Slider::new(&mut finish.grain, 0..=mapping::GRAIN_MAX))
    });
    ui.label(
        theme::muted(
            "Saturation above 100% makes what shows through more vivid, below makes it grey.",
        )
        .small(),
    );
    changed
}
