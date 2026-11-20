//! Presets, Shortcuts, Auto-pause and Settings: Windows Settings-style rows in cards.

use super::parts::*;
use super::window::{MainWindow, Message, Naming, GLASSES, THEMES};
use crate::mapping::{self, BlurStyle, Look, TintSource};
use crate::rules::{self, HotkeyAction};
use windows_reactor::*;

impl MainWindow {
    pub(super) fn presets_page(&self, context: &ViewContext<Self>) -> View {
        let header = page_header(
            "Presets",
            "Saved looks you can put on any app. Save one from an app's Look tab.",
        );
        if self.settings.presets.is_empty() {
            return page_frame(StackPanel::new().spacing(22.0).children((
                header,
                card(
                    Thickness::new(24.0, 32.0, 24.0, 32.0),
                    StackPanel::new()
                        .spacing(8.0)
                        .horizontal_alignment(HorizontalAlignment::Center)
                        .children((
                            glyph("\u{E734}", 32.0)
                                .foreground(ThemeBrush::SecondaryText)
                                .horizontal_alignment(HorizontalAlignment::Center),
                            strong("No presets").horizontal_alignment(HorizontalAlignment::Center),
                            caption("Open an app on the Apps page and use Save as preset.")
                                .horizontal_alignment(HorizontalAlignment::Center),
                        )),
                ),
            )));
        }
        // "Use on" puts a preset on the app picked on the Apps page.
        let target = self
            .selected
            .clone()
            .filter(|process| self.store.rule(process).is_some());
        let rows = self
            .settings
            .presets
            .iter()
            .enumerate()
            .map(|(index, preset)| {
                let mut buttons: Vec<View> = Vec::new();
                if let Some(process) = &target {
                    buttons.push(
                        Button::new()
                            .content(format!("Use on {process}"))
                            .on_click(context.message(Message::ApplyPreset(index)))
                            .into(),
                    );
                }
                let renaming = self.naming == Some(Naming::Rename(index));
                buttons.push(
                    Button::new()
                        .content("Rename…")
                        .on_click(context.message(Message::AskName(Naming::Rename(index))))
                        .content_dialog(
                            ContentDialog::new()
                                .title("Rename the preset")
                                .content(
                                    TextBox::new()
                                        .text(if renaming {
                                            self.name_draft.clone()
                                        } else {
                                            preset.name.clone()
                                        })
                                        .automation_name("Preset name")
                                        .on_text_changed(context.callback(Message::NameDraft)),
                                )
                                .primary_button_text("Rename")
                                .close_button_text("Cancel")
                                .default_button(ContentDialogButton::Primary)
                                .is_open(renaming)
                                .on_closed(context.callback(Message::NameClosed)),
                        )
                        .into(),
                );
                buttons.push(
                    Button::new()
                        .content("Delete…")
                        .on_click(context.message(Message::AskDeletePreset(index)))
                        .content_dialog(
                            ContentDialog::new()
                                .title(format!("Delete {}?", preset.name))
                                .content("Apps that use this look keep it.")
                                .primary_button_text("Delete")
                                .close_button_text("Cancel")
                                .default_button(ContentDialogButton::Close)
                                .is_open(self.asking_delete_preset == Some(index))
                                .on_closed(context.callback(Message::DeletePresetClosed)),
                        )
                        .into(),
                );
                Border::new()
                    .padding(Thickness::new(16.0, 12.0, 16.0, 12.0))
                    .content(
                        Grid::new()
                            .columns([GridLength::Pixel(56.0), GridLength::STAR, GridLength::Auto])
                            .column_spacing(12.0)
                            .children((
                                swatch(&preset.look),
                                StackPanel::new()
                                    .grid_column(1)
                                    .vertical_alignment(VerticalAlignment::Center)
                                    .children((
                                        strong(&preset.name),
                                        caption(&preset_summary(&preset.look)),
                                    )),
                                StackPanel::new()
                                    .grid_column(2)
                                    .orientation(Orientation::Horizontal)
                                    .spacing(8.0)
                                    .vertical_alignment(VerticalAlignment::Center)
                                    .children(buttons),
                            )),
                    )
                    .into()
            })
            .collect::<Vec<View>>();
        page_frame(
            StackPanel::new()
                .spacing(22.0)
                .max_width(1000.0)
                .children((header, section("Your presets", rows))),
        )
    }

    pub(super) fn shortcuts_page(&self, context: &ViewContext<Self>) -> View {
        let hotkeys = &self.settings.hotkeys;
        let problems = self.shared.hotkey_problems.get();
        let mut items = vec![row(
            "Use shortcuts",
            "Keys that work from any app, even while Blurman is in the tray.",
            switch(hotkeys.enabled, "Use shortcuts", context.callback(Message::UseShortcuts)),
        )];
        for action in HotkeyAction::ALL {
            let current = hotkeys.get(action).to_string();
            let recording = self.recording == Some(action);
            let mut record = Button::new()
                .min_width(140.0)
                .is_enabled(hotkeys.enabled)
                .automation_name(format!("Shortcut for {}", action.label()))
                .on_click(context.message(Message::Record(action)))
                .content(if recording {
                    "Press keys…".to_string()
                } else if current.is_empty() {
                    "Not set".to_string()
                } else {
                    current.clone()
                });
            if recording {
                record = record.style(ButtonStyle::Accent);
            }
            let mut controls: Vec<View> = Vec::new();
            if !current.is_empty() {
                controls.push(
                    Button::new()
                        .style(ButtonStyle::Subtle)
                        .is_enabled(hotkeys.enabled)
                        .automation_name(format!("Turn off the shortcut for {}", action.label()))
                        .on_click(context.message(Message::ClearShortcut(action)))
                        .content("Clear")
                        .into(),
                );
            }
            controls.push(record.into());
            let problem = problems
                .iter()
                .find(|(known, _)| *known == action)
                .map(|(_, problem)| problem.clone())
                .filter(|_| hotkeys.enabled);
            let under: View = match (&problem, recording) {
                (_, true) => caption("Press the new keys. Esc keeps the old ones; Backspace turns it off.")
                    .margin(Thickness::new(0.0, 4.0, 0.0, 0.0))
                    .into(),
                (Some(problem), false) => warning(problem)
                    .margin(Thickness::new(0.0, 4.0, 0.0, 0.0))
                    .into(),
                (None, false) => Border::new().into(),
            };
            items.push(row_with(
                action.label(),
                action_detail(action),
                under,
                StackPanel::new()
                    .orientation(Orientation::Horizontal)
                    .spacing(8.0)
                    .children(controls)
                    .into(),
            ));
        }
        page_frame(StackPanel::new().spacing(22.0).max_width(1000.0).children((
            page_header(
                "Shortcuts",
                "Click a shortcut, then press the new keys.",
            ),
            section("Shortcuts", items),
            caption(
                "A shortcut needs Ctrl, Alt or Win, so it never gets in the way of typing. F-keys work alone.",
            ),
        )))
    }

    pub(super) fn auto_pause_page(&self, context: &ViewContext<Self>) -> View {
        let auto = &self.settings.auto_pause;
        let now: View = match self.shared.auto_paused.get() {
            Some(reason) => InfoBar::new()
                .title("Auto-paused")
                .message(format!("{reason}. Every app is back to normal until that ends."))
                .severity(InfoBarSeverity::Warning)
                .is_open(true)
                .is_closable(false)
                .into(),
            None => InfoBar::new()
                .message("Not auto-paused right now.")
                .severity(InfoBarSeverity::Informational)
                .is_open(true)
                .is_closable(false)
                .into(),
        };
        let times = StackPanel::new()
            .orientation(Orientation::Horizontal)
            .spacing(8.0)
            .margin(Thickness::new(0.0, 10.0, 0.0, 0.0))
            .children((
                TextBox::new()
                    .width(90.0)
                    .text(auto.from.clone())
                    .placeholder_text("22:00")
                    .automation_name("From")
                    .on_text_changed(context.callback(Message::From)),
                TextBlock::new()
                    .text("and")
                    .vertical_alignment(VerticalAlignment::Center),
                TextBox::new()
                    .width(90.0)
                    .text(auto.until.clone())
                    .placeholder_text("07:00")
                    .automation_name("Until")
                    .on_text_changed(context.callback(Message::Until)),
            ));
        page_frame(StackPanel::new().spacing(22.0).max_width(1000.0).children((
            page_header(
                "Auto-pause",
                "While one of these is true, every app is back to normal. Your rules stay as they are, and apps frost again when it ends.",
            ),
            now,
            section(
                "Pause on its own",
                vec![
                    row(
                        "While battery saver is on",
                        "Saves the little power the glass uses.",
                        switch(auto.battery_saver, "While battery saver is on", context.callback(Message::BatterySaver)),
                    ),
                    row(
                        "During Do not disturb or presentation mode",
                        "Includes the Do not disturb Windows turns on for games and full-screen video.",
                        switch(auto.quiet, "During Do not disturb or presentation mode", context.callback(Message::Quiet)),
                    ),
                    row_with(
                        "Every day between",
                        "Times are 24-hour. A span past midnight, like 22:00 to 07:00, works.",
                        times.into(),
                        switch(auto.schedule, "Every day between these times", context.callback(Message::Schedule)),
                    ),
                ],
            ),
        )))
    }

    pub(super) fn settings_page(&self, context: &ViewContext<Self>) -> View {
        let appearance = &self.settings.appearance;
        let theme = THEMES
            .iter()
            .position(|name| *name == appearance.theme)
            .unwrap_or(0);
        let glass = GLASSES
            .iter()
            .position(|name| *name == appearance.window)
            .unwrap_or(0);
        let updates: Vec<View> = if crate::package::is_packaged() {
            vec![row(
                "Updates come from the Microsoft Store",
                "The Store installs new versions of Blurman on its own.",
                Border::new(),
            )]
        } else {
            vec![row(
                "Check for updates once a day",
                "Off unless you turn it on. Asks GitHub whether a newer Blurman is out and shows a link. Nothing is downloaded, and nothing else is sent anywhere.",
                StackPanel::new()
                    .orientation(Orientation::Horizontal)
                    .spacing(16.0)
                    .children((
                        Button::new()
                            .content("Check now")
                            .on_click(context.message(Message::CheckNow)),
                        switch(
                            self.settings.check_updates,
                            "Check for updates once a day",
                            context.callback(Message::Updates),
                        ),
                    )),
            ),
            row(
                "Get Blurman from the Microsoft Store",
                "Signed by Microsoft, and it updates itself.",
                Button::new()
                    .content("Open the Store")
                    .on_click(context.message(Message::OpenStore)),
            )]
        };
        let groups = StackPanel::new().spacing(22.0).max_width(1000.0).children((
            page_header(
                "Settings",
                "For Blurman itself. Each app's glass is set on the Apps page.",
            ),
            section(
                "Appearance",
                vec![
                    row(
                        "Theme",
                        "Light, dark, or whatever Windows is set to",
                        ComboBox::new()
                            .min_width(200.0)
                            .items_source(["Use Windows setting", "Light", "Dark"])
                            .selected_index(Some(theme))
                            .on_selection_changed(context.callback(Message::Theme)),
                    ),
                    row(
                        "Window glass",
                        "The glass behind this window. It stays glass while you use other apps.",
                        ComboBox::new()
                            .min_width(200.0)
                            .items_source(["Mica", "Mica Alt", "Acrylic"])
                            .selected_index(Some(glass))
                            .on_selection_changed(context.callback(Message::Glass)),
                    ),
                    row(
                        "Compact sidebar",
                        "Only the pages' icons",
                        switch(appearance.compact_sidebar, "Compact sidebar", context.callback(Message::Compact)),
                    ),
                ],
            ),
            section(
                "Startup and tray",
                vec![
                    row(
                        "Start with Windows",
                        "Starts quietly in the tray when you sign in, so your apps are frosted from the start.",
                        switch(self.startup, "Start with Windows", context.callback(Message::Startup)),
                    ),
                    row(
                        "Keep in tray when the window closes",
                        "Apps stay frosted. Click the tray icon to come back, or right-click it to pause, switch apps, or exit.",
                        switch(self.settings.close_to_tray, "Keep in tray when the window closes", context.callback(Message::Tray)),
                    ),
                ],
            ),
            section("Updates", updates),
            section(
                "Rules and presets",
                vec![
                    row(
                        "Where they are kept",
                        &rules::app_dir_on_disk().display().to_string(),
                        Button::new()
                            .on_click(context.message(Message::OpenFolder))
                            .content(with_glyph("\u{E838}", "Open folder")),
                    ),
                    row(
                        "Delete every rule",
                        "Every app goes back to normal. Presets and settings stay. Blurman asks first.",
                        Button::new()
                            .is_enabled(!self.store.rules.is_empty())
                            .content("Delete all rules…")
                            .on_click(context.message(Message::AskDeleteAll))
                            .content_dialog(
                                ContentDialog::new()
                                    .title("Delete every rule?")
                                    .content("Every app goes back to normal, and Blurman forgets how you frosted them. Your presets stay.")
                                    .primary_button_text("Delete all")
                                    .close_button_text("Cancel")
                                    .default_button(ContentDialogButton::Close)
                                    .is_open(self.asking_delete_all)
                                    .on_closed(context.callback(Message::DeleteAllClosed)),
                            ),
                    ),
                ],
            ),
            StackPanel::new()
                .orientation(Orientation::Horizontal)
                .spacing(4.0)
                .children((
                    caption(&format!(
                        "Blurman {} · Each frosted app is faded, and a blurred glass window sits right behind it. MIT license ·",
                        env!("CARGO_PKG_VERSION")
                    ))
                    .vertical_alignment(VerticalAlignment::Center),
                    HyperlinkButton::new()
                        .content("Source on GitHub")
                        .navigate_uri("https://github.com/xnixy/blurman")
                        .unwrap_or_else(|_| HyperlinkButton::new().content("Source on GitHub")),
                )),
        ));
        page_frame(groups)
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

/// A preset in a few words: its glass, how see-through, and what its finish changes.
fn preset_summary(look: &Look) -> String {
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
        parts.push(format!("milk {}", finish.milk));
    }
    if finish.contrast > 0 {
        parts.push(format!("contrast {}", finish.contrast));
    }
    if finish.grain > 0 {
        parts.push(format!("grain {}", finish.grain));
    }
    parts.join(", ")
}

/// A tile in a preset's color: a sample red through its saturation, milk and tint, and lighter
/// on one side the more it shows through. Theme and accent tints show what they are right now.
fn swatch(look: &Look) -> View {
    let finish = &look.finish;
    let tint = crate::glass::tint_now(finish);
    let base = [170.0f32, 60.0, 80.0];
    let grey = (base[0] + base[1] + base[2]) / 3.0;
    let saturation = finish.saturation as f32 / 100.0;
    let milk = finish.milk as f32 / 100.0 * 0.6;
    let amount = match look.style {
        BlurStyle::Acrylic => 0.55,
        BlurStyle::Frost => finish.tint_amount as f32 / 100.0,
    };
    let channel = |index: usize, tint: u8| {
        let saturated = (grey + (base[index] - grey) * saturation).clamp(0.0, 255.0);
        let milky = saturated + (242.0 - saturated) * milk;
        (milky + (tint as f32 - milky) * amount).round().clamp(0.0, 255.0) as u8
    };
    let color = Color::rgb(channel(0, tint.0), channel(1, tint.1), channel(2, tint.2));
    let glass = (look.transparency as f32 * 1.4 * 0.25).round() as u8;
    Border::new()
        .width(44.0)
        .height(44.0)
        .corner_radius(10.0)
        .background(color)
        .vertical_alignment(VerticalAlignment::Center)
        .content(
            Border::new()
                .width(22.0)
                .horizontal_alignment(HorizontalAlignment::Left)
                .corner_radius(10.0)
                .background(Color::argb(glass, 255, 255, 255)),
        )
        .into()
}
