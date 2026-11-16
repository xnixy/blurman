//! The Apps page: every app in a list pane, and an editor pane for the one you pick.

use super::widgets::{self, Icon, RowInfo};
use super::{BlurmanApp, Confirm, EditorTab, GAP, ONE_PANE_BELOW};
use crate::mapping::{self, BlurStyle, FocusLook, Look, Rgb, TintSource};
use crate::rules::Preset;
use crate::theme::{self, colors};
use egui::{Rect, RichText, Slider, Ui};
use std::sync::atomic::Ordering;

const PRESET_NAME: &str = "preset-name";
/// Tint amount given when you switch to the theme or accent tint from a tint of nothing.
const TINT_ON_PICK: u8 = 25;

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
    /// The list and the editor side by side, or one at a time where the window is narrow.
    pub(super) fn apps_page(&mut self, ui: &mut Ui, rect: Rect) {
        if rect.width() < ONE_PANE_BELOW {
            if self.selected.is_some() && self.detail_open {
                widgets::pane(ui, rect, |ui| self.editor(ui, true));
            } else {
                widgets::pane(ui, rect, |ui| self.app_list(ui));
            }
            return;
        }
        let list_w = (rect.width() * 0.38).clamp(200.0, 320.0);
        let list = Rect::from_min_max(rect.min, egui::pos2(rect.left() + list_w, rect.bottom()));
        let detail = Rect::from_min_max(egui::pos2(list.right() + GAP, rect.top()), rect.max);
        widgets::pane(ui, list, |ui| self.app_list(ui));
        widgets::pane(ui, detail, |ui| self.editor(ui, false));
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
        let lists = self.entries();
        let mut picked = None;
        let mut toggled = None;
        let width = ui.available_width();
        egui::ScrollArea::vertical()
            .id_salt("apps")
            .auto_shrink([false, false])
            .drag_to_scroll(false)
            .show(ui, |ui| {
                // Rows are pinned to the pane: one over-wide row would widen them all.
                ui.set_width(width);
                ui.spacing_mut().item_spacing.y = 2.0;
                let titles = ["Frosted", "Off", "Open apps"];
                let mut first = true;
                for (list, title) in lists.iter().zip(titles) {
                    if list.is_empty() {
                        continue;
                    }
                    if !first {
                        ui.add_space(8.0);
                    }
                    first = false;
                    ui.horizontal(|ui| {
                        ui.add_space(10.0);
                        widgets::group_label(ui, title);
                    });
                    for entry in list {
                        let icon = self
                            .icons
                            .get(ui.ctx(), &entry.process, entry.path.as_deref());
                        let mut enabled = entry.enabled;
                        let response = widgets::app_row(
                            ui,
                            &mut RowInfo {
                                process: &entry.process,
                                subtitle: &entry.subtitle,
                                icon: icon.as_ref(),
                                selected: self.selected.as_deref() == Some(entry.process.as_str()),
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
                    ui.add_space(ui.available_height() * 0.3);
                    let text = if self.search.trim().is_empty() {
                        "No app windows are open."
                    } else {
                        "No app matches that search."
                    };
                    widgets::empty(ui, Icon::APPS, text);
                }
            });
        if let Some((process, on)) = toggled {
            self.set_enabled(&process, on);
        }
        if let Some(process) = picked {
            self.select(process);
        }
    }

    /// The app you picked: its name and switch, then its look, finish, and behavior.
    /// `back` adds the arrow that returns to the list, where only one pane shows.
    fn editor(&mut self, ui: &mut Ui, back: bool) {
        let Some(process) = self.selected.clone() else {
            self.empty_editor(ui);
            return;
        };
        let group = self
            .groups
            .iter()
            .find(|group| group.process.eq_ignore_ascii_case(&process))
            .cloned();
        let ruled = self.selected_rule().map(|rule| rule.enabled);
        let width = ui.available_width();
        egui::ScrollArea::vertical()
            .id_salt("editor")
            .auto_shrink([false, false])
            .drag_to_scroll(false)
            .show(ui, |ui| {
                ui.set_width(width);
                let subtitle = group
                    .as_ref()
                    .map(|group| group.sample_title.clone())
                    .unwrap_or_else(|| "Not running".into());
                let icon = self.icons.get(
                    ui.ctx(),
                    &process,
                    group.as_ref().and_then(|group| group.path.as_deref()),
                );
                let mut go_back = false;
                let mut switched = None;
                let mut frost = false;
                widgets::trailing(
                    ui,
                    |ui| match ruled {
                        Some(mut on) => {
                            if widgets::switch(ui, &mut on, &format!("Frost {process}")).changed()
                            {
                                switched = Some(on);
                            }
                            ui.label(theme::muted(if on { "Frosted" } else { "Off" }));
                        }
                        None => frost = widgets::primary(ui, "Frost it").clicked(),
                    },
                    |ui| {
                        ui.horizontal(|ui| {
                            if back {
                                go_back =
                                    widgets::round_icon(ui, Icon::BACK, "Back to the apps").clicked();
                            }
                            widgets::app_icon(ui, icon.as_ref(), &process, 40.0);
                            let text_w = ui.available_width().max(40.0);
                            ui.vertical(|ui| {
                                ui.set_width(text_w);
                                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.label(
                                    RichText::new(&process)
                                        .font(theme::semibold(16.0))
                                        .color(colors().text),
                                )
                                .on_hover_text(&process);
                                widgets::window_title(ui, &subtitle);
                            });
                        });
                    },
                );
                if go_back {
                    self.detail_open = false;
                }
                if let Some(on) = switched {
                    self.set_enabled(&process, on);
                }
                if frost {
                    self.frost_selected();
                }
                if let Some(group) = &group {
                    if group.elevated {
                        note(
                            ui,
                            "This app runs as administrator. Run Blurman as administrator to frost it.",
                        );
                    } else if group.fullscreen {
                        note(
                            ui,
                            "This app covers the screen, so Blurman leaves it alone until it is windowed.",
                        );
                    }
                } else if ruled.is_some() {
                    ui.add(
                        egui::Label::new(theme::detail(
                            "Not open now. Its windows are frosted when it starts.",
                        ))
                        .wrap(),
                    );
                }
                ui.add_space(8.0);
                widgets::segmented(
                    ui,
                    &mut self.editor_tab,
                    &[
                        (
                            EditorTab::Look,
                            "Look",
                            "Style, transparency, blur, and presets.",
                        ),
                        (EditorTab::Finish, "Finish", "Tint, saturation, and grain."),
                        (
                            EditorTab::Behavior,
                            "Behavior",
                            "How it looks while you use it, and which windows.",
                        ),
                    ],
                );
                widgets::divider(ui);
                match self.editor_tab {
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
                }
                if ruled.is_some() {
                    widgets::divider(ui);
                    let remove = widgets::setting_row(
                        ui,
                        "Changes apply as you make them.",
                        "Removing the rule forgets this app's look and behavior.",
                        |ui| widgets::danger(ui, "Remove rule…").clicked(),
                    );
                    if remove {
                        self.confirm = Some(Confirm::RemoveRule(process.clone()));
                    }
                }
            });
    }

    /// What the editor pane shows before an app is picked.
    fn empty_editor(&mut self, ui: &mut Ui) {
        ui.add_space((ui.available_height() * 0.5 - 70.0).max(0.0));
        widgets::empty(
            ui,
            Icon::APPS,
            "Choose an app from the list, or click the window you mean.",
        );
        ui.add_space(6.0);
        let mut pick = false;
        ui.vertical_centered(|ui| {
            pick = widgets::primary(ui, "Pick a window")
                .on_hover_text(
                    "Blurman steps aside. Click any window to choose its app, or press Esc.",
                )
                .clicked();
        });
        if pick {
            let ctx = ui.ctx().clone();
            self.pick_window(&ctx);
        }
    }

    /// Style, transparency, blur, and presets. Returns whether the look changed.
    fn look_tab(&mut self, ui: &mut Ui) -> bool {
        let fallback = self.shared.fallback.load(Ordering::SeqCst);
        let mut moved = false;
        ui.label(theme::muted("Style"));
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
        let about = if fallback {
            "This PC uses system acrylic, so the blur slider sets how milky the glass is."
        } else {
            match self.look.style {
                BlurStyle::Frost => "Blurs whatever is behind the window.",
                BlurStyle::Acrylic => "Milky system glass. The slider sets how milky it is.",
            }
        };
        ui.add(egui::Label::new(theme::detail(about)).wrap());
        ui.add_space(4.0);
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
        widgets::divider(ui);
        ui.label(theme::muted("Presets"));
        moved | self.presets_row(ui)
    }

    /// One click puts a saved look on the sliders.
    fn presets_row(&mut self, ui: &mut Ui) -> bool {
        let mut picked = None;
        let mut save = false;
        let mut cancel = false;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
            for preset in &self.settings.presets {
                if widgets::chip(ui, preset.look == self.look, &preset.name)
                    .on_hover_text(preset_summary(&preset.look))
                    .clicked()
                {
                    picked = Some(preset.look);
                }
            }
            match &mut self.naming_preset {
                None => {
                    if widgets::secondary(ui, "Save as preset…")
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
                            .margin(widgets::FIELD_MARGIN)
                            .desired_width(110.0),
                    );
                    widgets::name(ui, &field, "Preset name");
                    let entered =
                        field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if widgets::primary(ui, "Save").clicked() || entered {
                        save = true;
                    }
                    if widgets::secondary(ui, "Cancel").clicked()
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
        ui.label(theme::muted("While you use it"));
        changed |= widgets::segmented(
            ui,
            &mut self.focus,
            &[
                (FocusLook::Same, "Same", ""),
                (FocusLook::Lighter, "Lighter", ""),
                (FocusLook::Solid, "Solid", ""),
            ],
        );
        ui.add(
            egui::Label::new(theme::detail(match self.focus {
                FocusLook::Same => "Stays frosted while it has focus.",
                FocusLook::Lighter => "Half as see-through while it has focus.",
                FocusLook::Solid => {
                    "Fully solid while it has focus, so text is easy to read. Frosted again when you move on."
                }
            }))
            .wrap(),
        );
        widgets::divider(ui);
        ui.label(theme::muted("Which windows"));
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
        ui.add(
            egui::Label::new(theme::detail(
                "Separate titles with |. Use * as a wildcard, like *- YouTube*. Leave both empty to frost every window.",
            ))
            .wrap(),
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
    ui.add(egui::Label::new(RichText::new(text).size(11.5).color(colors().warn)).wrap());
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
    if finish.milk > 0 {
        parts.push(format!("milk {}%", finish.milk));
    }
    if finish.contrast > 0 {
        parts.push(format!("contrast {}", finish.contrast));
    }
    if finish.grain > 0 {
        parts.push(format!("grain {}", finish.grain));
    }
    parts.join(" · ")
}

/// Tint, saturation, contrast, and grain. Acrylic only takes the tint color; the rest needs frost.
pub(super) fn finish_controls(ui: &mut Ui, look: &mut Look, milky: bool) -> bool {
    let mut changed = false;
    let finish = &mut look.finish;
    ui.label(theme::muted("Tint"));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        let was = finish.tint_source;
        let picked = widgets::segmented_in(
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
        // Frost with no tint amount would show no change at all for the new source.
        if picked && was == TintSource::Custom && finish.tint_amount == 0 {
            finish.tint_amount = TINT_ON_PICK;
        }
        changed |= picked;
        if finish.tint_source == TintSource::Custom {
            let mut rgb = [finish.tint.0, finish.tint.1, finish.tint.2];
            let button = egui::color_picker::color_edit_button_srgb(ui, &mut rgb);
            widgets::name(ui, &button, "Tint color");
            if button.changed() {
                finish.tint = Rgb(rgb[0], rgb[1], rgb[2]);
                changed = true;
            }
        }
    });
    ui.add_space(4.0);
    if milky {
        ui.add(
            egui::Label::new(theme::detail(
                "Acrylic is colored with the tint, and Milkiness sets how much. Saturation, milk, contrast, and grain need Frost.",
            ))
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
    changed |= widgets::slider_row(ui, "Milk", |ui| {
        ui.add(Slider::new(&mut finish.milk, 0..=mapping::MILK_MAX).suffix("%"))
    });
    changed |= widgets::slider_row(ui, "Contrast", |ui| {
        ui.add(Slider::new(&mut finish.contrast, 0..=mapping::CONTRAST_MAX))
    });
    changed |= widgets::slider_row(ui, "Grain", |ui| {
        ui.add(Slider::new(&mut finish.grain, 0..=mapping::GRAIN_MAX))
    });
    ui.add(
        egui::Label::new(theme::detail(
            "Saturation above 100% makes what shows through more vivid, below makes it grey. For an app in light mode, Milk keeps the glass bright instead of grey, and keeps its color.",
        ))
        .wrap(),
    );
    changed
}
