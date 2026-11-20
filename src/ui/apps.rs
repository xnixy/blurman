//! The Apps page: every app in one list (frosted first, then switched off, then the rest of what
//! is open), and the picked app's glass: a live preview, then Look, Finish and Behavior.

use super::parts::*;
use super::window::{Entry, MainWindow, Message, Naming, Tab, FOCUSES, TINTS};
use crate::mapping::{self, BlurStyle, TintSource};
use crate::rules::Rule;
use std::rc::Rc;
use windows_reactor::*;

impl MainWindow {
    pub(super) fn apps_page(&self, context: &ViewContext<Self>) -> View {
        let header = Grid::new()
            .columns([GridLength::STAR, GridLength::Auto])
            .column_spacing(16.0)
            .children((
                page_header(
                    "Apps",
                    "Frosted apps first, then the ones you switched off, then everything else that is open.",
                )
                .vertical_alignment(VerticalAlignment::Bottom),
                StackPanel::new()
                    .orientation(Orientation::Horizontal)
                    .spacing(8.0)
                    .vertical_alignment(VerticalAlignment::Bottom)
                    .grid_column(1)
                    .children((
                        AutoSuggestBox::new()
                            .width(240.0)
                            .text(self.search.clone())
                            .placeholder_text("Search apps")
                            .query_icon(Icon::symbol(Symbol::Find))
                            .on_text_changed(context.callback(Message::Search)),
                        Button::new()
                            .automation_name("Pick a window")
                            .on_click(context.message(Message::PickWindow))
                            .content(with_glyph("\u{E7C9}", "Pick a window")),
                    )),
            ));
        let lists = self.entries();
        let mut list: Vec<View> = Vec::new();
        for (index, (label, entries)) in ["Frosted", "Switched off", "Open now"]
            .into_iter()
            .zip(lists.iter())
            .enumerate()
        {
            if entries.is_empty() {
                continue;
            }
            list.push(
                TextBlock::new()
                    .text(format!("{label} · {}", entries.len()))
                    .font_size(12.0)
                    .font_weight(FontWeight::SEMI_BOLD)
                    .foreground(ThemeBrush::SecondaryText)
                    .margin(Thickness::new(16.0, 14.0, 16.0, 4.0))
                    .into(),
            );
            let selected = self.selected.as_ref().and_then(|process| {
                entries
                    .iter()
                    .position(|entry| entry.process.eq_ignore_ascii_case(process))
            });
            list.push(
                ListView::new()
                    .selection_mode(ListViewSelectionMode::Single)
                    .selected_index(selected)
                    .items(
                        entries
                            .iter()
                            .map(|entry| {
                                DataItem::new(entry.process.clone(), entry.process.clone())
                                    .content(self.row(entry, context))
                            })
                            .collect::<Vec<_>>(),
                    )
                    .on_selection_changed(
                        context.callback(move |pick: Option<usize>| Message::Select(index, pick)),
                    )
                    .into(),
            );
        }
        if list.is_empty() {
            list.push(
                caption(&format!(
                    "No open app or rule matches “{}”.",
                    self.search.trim()
                ))
                .margin(Thickness::new(16.0, 20.0, 16.0, 20.0))
                .into(),
            );
        }
        let panes = Grid::new()
            .columns([GridLength::Pixel(340.0), GridLength::STAR])
            .column_spacing(24.0)
            .children((
                card(
                    Thickness::new(0.0, 0.0, 0.0, 6.0),
                    StackPanel::new().children(list),
                )
                .vertical_alignment(VerticalAlignment::Top),
                Border::new().grid_column(1).content(self.editor(context)),
            ));
        page_frame(StackPanel::new().spacing(20.0).children((header, panes)))
    }

    fn row(&self, entry: &Entry, context: &ViewContext<Self>) -> View {
        let words = StackPanel::new()
            .vertical_alignment(VerticalAlignment::Center)
            .grid_column(1)
            .children((
                TextBlock::new()
                    .text(entry.process.clone())
                    .text_trimming(TextTrimming::CharacterEllipsis),
                TextBlock::new()
                    .text(entry.subtitle.clone())
                    .font_size(12.0)
                    .foreground(ThemeBrush::SecondaryText)
                    .text_trimming(TextTrimming::CharacterEllipsis),
            ));
        let tag: View = if entry.elevated {
            admin_tag().grid_column(2).into()
        } else {
            Border::new().grid_column(2).into()
        };
        let switch: View = match entry.enabled {
            Some(on) => {
                let process = entry.process.clone();
                ToggleSwitch::new()
                    .is_on(on)
                    .on_content("")
                    .off_content("")
                    .min_width(0.0)
                    .automation_name(format!("Frost {}", entry.process))
                    .on_toggled(
                        context.callback(move |on: bool| Message::Enabled(process.clone(), on)),
                    )
                    .grid_column(3)
                    .into()
            }
            None => Border::new().grid_column(3).into(),
        };
        Grid::new()
            .columns([
                GridLength::Pixel(40.0),
                GridLength::STAR,
                GridLength::Auto,
                GridLength::Auto,
            ])
            .column_spacing(8.0)
            .min_height(52.0)
            .automation_name(format!("{}, {}", entry.process, entry.subtitle))
            .children((
                app_icon(self.icon_of(entry.path.as_ref()), 32.0)
                    .vertical_alignment(VerticalAlignment::Center),
                words,
                tag,
                switch,
            ))
            .into()
    }

    fn editor(&self, context: &ViewContext<Self>) -> View {
        let Some(process) = self.selected.clone() else {
            return StackPanel::new()
                .spacing(8.0)
                .margin(Thickness::new(0.0, 80.0, 0.0, 0.0))
                .horizontal_alignment(HorizontalAlignment::Center)
                .children((
                    glyph("\u{E71D}", 40.0)
                        .foreground(ThemeBrush::SecondaryText)
                        .horizontal_alignment(HorizontalAlignment::Center),
                    subtitle_text("Pick an app").horizontal_alignment(HorizontalAlignment::Center),
                    caption("Choose an app on the left to frost it or change its glass.")
                        .horizontal_alignment(HorizontalAlignment::Center),
                ))
                .into();
        };
        let group = self.group(&process);
        let detail = match group {
            Some(group) if group.windows == 1 => format!("{} · 1 window open", group.process),
            Some(group) => format!("{} · {} windows open", group.process, group.windows),
            None => format!("{process} · not running"),
        };
        let rule = self.store.rule(&process);
        let toggle: View = match rule {
            Some(rule) => {
                let target = process.clone();
                StackPanel::new()
                    .orientation(Orientation::Horizontal)
                    .spacing(10.0)
                    .vertical_alignment(VerticalAlignment::Center)
                    .grid_column(2)
                    .children((
                        TextBlock::new()
                            .text(if rule.enabled { "Frosted" } else { "Off" })
                            .vertical_alignment(VerticalAlignment::Center),
                        ToggleSwitch::new()
                            .is_on(rule.enabled)
                            .on_content("")
                            .off_content("")
                            .min_width(0.0)
                            .automation_name(format!("Frost {process}"))
                            .on_toggled(
                                context
                                    .callback(move |on: bool| Message::Enabled(target.clone(), on)),
                            ),
                    ))
                    .into()
            }
            None => Border::new().grid_column(2).into(),
        };
        let header = Grid::new()
            .columns([GridLength::Pixel(62.0), GridLength::STAR, GridLength::Auto])
            .children((
                app_icon(
                    self.icon_of(group.and_then(|group| group.path.as_ref())),
                    48.0,
                )
                .vertical_alignment(VerticalAlignment::Center),
                StackPanel::new()
                    .vertical_alignment(VerticalAlignment::Center)
                    .grid_column(1)
                    .children((subtitle_text(&process), caption(&detail))),
                toggle,
            ));
        let Some(rule) = rule else {
            let elevated = group.is_some_and(|group| group.elevated);
            let mut parts: Vec<View> = Vec::new();
            if elevated {
                parts.push(
                    InfoBar::new()
                        .title("Runs as administrator")
                        .message(
                            "Blurman can frost it only while Blurman runs as administrator too.",
                        )
                        .severity(InfoBarSeverity::Warning)
                        .is_open(true)
                        .is_closable(false)
                        .into(),
                );
            }
            parts.push(strong("Not frosted").into());
            parts.push(
                caption(&format!(
                    "Frost it to fade its windows and put glass behind them. New {process} windows get the same look as they open."
                ))
                .into(),
            );
            parts.push(
                Button::new()
                    .style(ButtonStyle::Accent)
                    .is_enabled(!elevated)
                    .on_click(context.message(Message::Frost(process.clone())))
                    .content(format!("Frost {process}"))
                    .into(),
            );
            parts.push(caption("You can change its look right after.").into());
            return StackPanel::new()
                .spacing(16.0)
                .children((
                    header,
                    card(
                        Thickness::new(24.0, 20.0, 24.0, 24.0),
                        StackPanel::new().spacing(12.0).children(parts),
                    ),
                ))
                .into();
        };
        let tabs = SelectorBar::new()
            .items((
                SelectorBarItem::new()
                    .text("Look")
                    .is_selected(self.tab == Tab::Look),
                SelectorBarItem::new()
                    .text("Finish")
                    .is_selected(self.tab == Tab::Finish),
                SelectorBarItem::new()
                    .text("Behavior")
                    .is_selected(self.tab == Tab::Behavior),
            ))
            .on_selected_text_changed(context.callback(Message::Tab));
        let panel = match self.tab {
            Tab::Look => self.look_rows(rule, context),
            Tab::Finish => self.finish_rows(rule, context),
            Tab::Behavior => self.behavior_rows(rule, context),
        };
        let remove = StackPanel::new()
            .orientation(Orientation::Horizontal)
            .spacing(12.0)
            .children((
                Button::new()
                    .content("Remove rule…")
                    .on_click(context.message(Message::AskDelete))
                    .content_dialog(
                        ContentDialog::new()
                            .title(format!("Remove the rule for {process}?"))
                            .content(
                                "The app goes back to normal. You can frost it again any time.",
                            )
                            .primary_button_text("Remove")
                            .close_button_text("Cancel")
                            .default_button(ContentDialogButton::Close)
                            .is_open(self.asking_delete)
                            .on_closed(context.callback(Message::DeleteClosed)),
                    ),
                caption(&format!(
                    "{process} goes back to normal. Blurman asks first."
                ))
                .vertical_alignment(VerticalAlignment::Center),
            ));
        StackPanel::new()
            .spacing(16.0)
            .max_width(760.0)
            .horizontal_alignment(HorizontalAlignment::Stretch)
            .children((
                header,
                self.preview_block(rule),
                tabs,
                card(Thickness::new(0.0, 0.0, 0.0, 0.0), panel),
                remove,
            ))
            .into()
    }

    fn look_rows(&self, rule: &Rule, context: &ViewContext<Self>) -> View {
        let look = rule.look;
        let acrylic = look.style == BlurStyle::Acrylic;
        let preset = self
            .settings
            .presets
            .iter()
            .position(|preset| preset.look == look);
        let naming = matches!(self.naming, Some(Naming::Save));
        rows(vec![
            row(
                "Glass",
                "Frost blurs what is behind the app. Acrylic is Windows' own milky glass.",
                RadioButtons::new()
                    .automation_name("Glass")
                    .width(260.0)
                    .items_source(["Frost", "Acrylic"])
                    .max_columns(2)
                    .selected_index(Some(if acrylic { 1 } else { 0 }))
                    .on_selection_changed(context.callback(Message::Style)),
            ),
            row(
                "Transparency",
                "How much shows through. Text fades too, so it stops at 70%.",
                slider(
                    look.transparency,
                    mapping::TRANSPARENCY_MIN,
                    mapping::TRANSPARENCY_MAX,
                    "%",
                    "Transparency",
                    context.callback(Message::Transparency),
                ),
            ),
            row(
                if acrylic { "Milkiness" } else { "Blur" },
                if acrylic {
                    "How milky the acrylic is."
                } else {
                    "How soft what shows through is."
                },
                slider(
                    look.blur,
                    mapping::BLUR_MIN,
                    mapping::BLUR_MAX,
                    "",
                    if acrylic { "Milkiness" } else { "Blur" },
                    context.callback(Message::Blur),
                ),
            ),
            row(
                "Preset",
                "Start from a saved look, or save this one to use on other apps.",
                StackPanel::new()
                    .orientation(Orientation::Horizontal)
                    .spacing(8.0)
                    .children((
                        ComboBox::new()
                            .automation_name("Preset")
                            .min_width(180.0)
                            .items_source(
                                self.settings
                                    .presets
                                    .iter()
                                    .map(|preset| preset.name.clone()),
                            )
                            .selected_index(preset)
                            .placeholder_text("Your own look")
                            .on_selection_changed(context.callback(Message::Preset)),
                        Button::new()
                            .content("Save as preset…")
                            .on_click(context.message(Message::AskName(Naming::Save)))
                            .content_dialog(
                                ContentDialog::new()
                                    .title("Save as a preset")
                                    .content(
                                        TextBox::new()
                                            .text(self.name_draft.clone())
                                            .placeholder_text("Name")
                                            .automation_name("Preset name")
                                            .on_text_changed(context.callback(Message::NameDraft)),
                                    )
                                    .primary_button_text("Save")
                                    .close_button_text("Cancel")
                                    .default_button(ContentDialogButton::Primary)
                                    .is_open(naming)
                                    .on_closed(context.callback(Message::NameClosed)),
                            ),
                    )),
            ),
        ])
    }

    fn finish_rows(&self, rule: &Rule, context: &ViewContext<Self>) -> View {
        let finish = rule.look.finish;
        let source = TINTS
            .iter()
            .position(|source| *source == finish.tint_source);
        // The tint's color on its row, and how much of it shows on the next: side by side they
        // leave the row's words no room in the editor's column.
        let mut tint_controls: Vec<View> = vec![ComboBox::new()
            .automation_name("Tint")
            .min_width(150.0)
            .items_source(["Your color", "Windows theme", "Accent color"])
            .selected_index(source)
            .on_selection_changed(context.callback(Message::TintSource))
            .into()];
        if finish.tint_source == TintSource::Custom {
            let color = Color::rgb(finish.tint.0, finish.tint.1, finish.tint.2);
            let draft = Color::rgb(self.tint_draft.0, self.tint_draft.1, self.tint_draft.2);
            tint_controls.push(
                Button::new()
                    .automation_name("Pick the tint color")
                    .on_click(context.message(Message::AskTint))
                    .content(
                        Border::new()
                            .width(20.0)
                            .height(20.0)
                            .corner_radius(4.0)
                            .border_brush(ThemeBrush::CardStroke)
                            .border_thickness(1.0)
                            .background(color),
                    )
                    .content_dialog(
                        ContentDialog::new()
                            .title("Tint color")
                            .content(
                                ColorPicker::new()
                                    .color(draft)
                                    .is_alpha_enabled(false)
                                    .on_color_changed(context.callback(Message::TintColor)),
                            )
                            .primary_button_text("Use it")
                            .close_button_text("Cancel")
                            .default_button(ContentDialogButton::Primary)
                            .is_open(self.asking_tint)
                            .on_closed(context.callback(Message::TintClosed)),
                    )
                    .into(),
            );
        }
        rows(vec![
            row(
                "Tint",
                "A color laid over the glass: one you pick, or the Windows theme or accent color.",
                StackPanel::new()
                    .orientation(Orientation::Horizontal)
                    .spacing(8.0)
                    .children(tint_controls),
            ),
            row(
                "Tint amount",
                "How much of the color shows. None at 0.",
                slider(
                    finish.tint_amount,
                    0,
                    mapping::TINT_MAX,
                    "%",
                    "Tint amount",
                    context.callback(Message::TintAmount),
                ),
            ),
            row(
                "Saturation",
                "Grey at 0, as it is at 100, vivid at 200.",
                slider(
                    finish.saturation,
                    mapping::SATURATION_MIN,
                    mapping::SATURATION_MAX,
                    "",
                    "Saturation",
                    context.callback(Message::Saturation),
                ),
            ),
            row(
                "Milk",
                "Lifts what shows through to a bright, milky glass and keeps its color, for apps in light mode.",
                slider(finish.milk, 0, mapping::MILK_MAX, "", "Milk", context.callback(Message::Milk)),
            ),
            row(
                "Contrast",
                "Punchier glass.",
                slider(
                    finish.contrast,
                    0,
                    mapping::CONTRAST_MAX,
                    "",
                    "Contrast",
                    context.callback(Message::Contrast),
                ),
            ),
            row(
                "Grain",
                "Fine noise, like real frosted glass. It shows on the app, not in the preview.",
                slider(finish.grain, 0, mapping::GRAIN_MAX, "", "Grain", context.callback(Message::Grain)),
            ),
        ])
    }

    fn behavior_rows(&self, rule: &Rule, context: &ViewContext<Self>) -> View {
        let focus = FOCUSES.iter().position(|focus| *focus == rule.focus);
        let chips = |titles: &[String], remove: fn(usize) -> Message, verb: &str| -> View {
            StackPanel::new()
                .orientation(Orientation::Horizontal)
                .spacing(6.0)
                .margin(Thickness::new(0.0, 10.0, 0.0, 0.0))
                .children(
                    titles
                        .iter()
                        .enumerate()
                        .map(|(index, title)| {
                            Button::new()
                                .automation_name(format!("{verb} {title}"))
                                .on_click(context.message(remove(index)))
                                .content(
                                    StackPanel::new()
                                        .orientation(Orientation::Horizontal)
                                        .spacing(8.0)
                                        .children((
                                            TextBlock::new().text(title.clone()),
                                            glyph("\u{E711}", 10.0)
                                                .vertical_alignment(VerticalAlignment::Center),
                                        )),
                                )
                                .into()
                        })
                        .collect::<Vec<View>>(),
                )
                .into()
        };
        let adder =
            |draft: &str, on_text: fn(Rc<str>) -> Message, add: Message, label: &str| -> View {
                StackPanel::new()
                    .orientation(Orientation::Horizontal)
                    .spacing(8.0)
                    .children((
                        TextBox::new()
                            .width(180.0)
                            .text(draft.to_string())
                            .placeholder_text("Title contains…")
                            .automation_name(label)
                            .on_text_changed(context.callback(on_text)),
                        Button::new()
                            .content("Add")
                            .automation_name(format!("Add {}", label.to_lowercase()))
                            .on_click(context.message(add)),
                    ))
                    .into()
            };
        rows(vec![
            row_under(
                "While you use it",
                "Lighter or solid while the app has focus, fading back to frosted when you move on.",
                RadioButtons::new()
                    .automation_name("While you use it")
                    .width(380.0)
                    .items_source(["Same", "Lighter", "Solid"])
                    .max_columns(3)
                    .selected_index(focus)
                    .on_selection_changed(context.callback(Message::Focus)),
            ),
            row_with(
                "Only these windows",
                "Frost only windows whose title has one of these words. Empty frosts every window.",
                chips(&rule.only_titles, Message::RemoveOnly, "Stop limiting to"),
                adder(
                    &self.only_draft,
                    Message::OnlyDraft,
                    Message::AddOnly,
                    "Title for only these windows",
                ),
            ),
            row_with(
                "Skip these windows",
                "Leave windows with these words in their title solid, like every Chrome window except YouTube. Titles are checked again as they change.",
                chips(&rule.skip_titles, Message::RemoveSkip, "Stop skipping"),
                adder(&self.skip_draft, Message::SkipDraft, Message::AddSkip, "Title to skip"),
            ),
        ])
    }

    /// Sample shapes behind a pane of the app's glass (Blurman's own frost brush, drawn by WinUI),
    /// with the app faded over the pane as much as its transparency says.
    fn preview_block(&self, rule: &Rule) -> View {
        let (desk, app_fill, line) = if self.dark() {
            (
                Color::rgb(0x22, 0x31, 0x4D),
                Color::rgb(0x20, 0x20, 0x20),
                Color::rgb(0x4A, 0x4F, 0x57),
            )
        } else {
            (
                Color::rgb(0xB9, 0xC9, 0xE4),
                Color::rgb(0xFF, 0xFF, 0xFF),
                Color::rgb(0xC9, 0xCE, 0xD6),
            )
        };
        let ink = Color::rgb(0x1F, 0x29, 0x37);
        let blob = |size: f64, color: Color| Ellipse::new().width(size).height(size).fill(color);
        let bar = |width: f64, color: Color| {
            Border::new()
                .width(width)
                .height(9.0)
                .corner_radius(4.5)
                .background(color)
        };
        let app = Border::new()
            .grid_column(1)
            .margin(Thickness::new(0.0, 22.0, 0.0, 0.0))
            .corner_radius(8.0)
            .background(app_fill)
            .opacity(1.0 - rule.look.transparency as f64 / 100.0)
            .padding(Thickness::new(16.0, 14.0, 16.0, 0.0))
            .content(
                StackPanel::new().spacing(10.0).children((
                    Grid::new()
                        .columns([GridLength::STAR, GridLength::Auto])
                        .children((
                            bar(64.0, line).horizontal_alignment(HorizontalAlignment::Left),
                            bar(30.0, line).grid_column(1),
                        )),
                    bar(240.0, line).horizontal_alignment(HorizontalAlignment::Left),
                    bar(180.0, line).horizontal_alignment(HorizontalAlignment::Left),
                    bar(210.0, line).horizontal_alignment(HorizontalAlignment::Left),
                )),
            );
        Border::new()
            .height(150.0)
            .corner_radius(7.0)
            .border_brush(ThemeBrush::CardStroke)
            .border_thickness(1.0)
            .background(desk)
            .automation_name(format!("Preview of the glass behind {}", rule.process))
            .content(
                Grid::new()
                    .columns([
                        GridLength::Star(1.0),
                        GridLength::Star(4.0),
                        GridLength::Star(1.0),
                    ])
                    .children((
                        blob(120.0, Color::rgb(0xF5, 0x9E, 0x0B))
                            .grid_column_span(2)
                            .horizontal_alignment(HorizontalAlignment::Left)
                            .vertical_alignment(VerticalAlignment::Top)
                            .margin(Thickness::new(28.0, 8.0, 0.0, 0.0)),
                        blob(96.0, Color::rgb(0x14, 0xB8, 0xA6))
                            .grid_column(1)
                            .horizontal_alignment(HorizontalAlignment::Center)
                            .vertical_alignment(VerticalAlignment::Bottom)
                            .margin(Thickness::new(80.0, 0.0, 0.0, 6.0)),
                        blob(136.0, Color::rgb(0x8B, 0x5C, 0xF6))
                            .grid_column(1)
                            .grid_column_span(2)
                            .horizontal_alignment(HorizontalAlignment::Right)
                            .vertical_alignment(VerticalAlignment::Top)
                            .margin(Thickness::new(0.0, 6.0, 20.0, 0.0)),
                        bar(200.0, ink)
                            .opacity(0.55)
                            .grid_column(1)
                            .horizontal_alignment(HorizontalAlignment::Left)
                            .vertical_alignment(VerticalAlignment::Top)
                            .margin(Thickness::new(30.0, 34.0, 0.0, 0.0)),
                        bar(140.0, ink)
                            .opacity(0.55)
                            .grid_column(1)
                            .horizontal_alignment(HorizontalAlignment::Left)
                            .vertical_alignment(VerticalAlignment::Top)
                            .margin(Thickness::new(60.0, 100.0, 0.0, 0.0)),
                        // The glass: WinUI puts the preview's visual on this element.
                        Grid::new()
                            .element_ref(&self.preview_host)
                            .grid_column(1)
                            .margin(Thickness::new(0.0, 22.0, 0.0, 0.0)),
                        app,
                        Border::new()
                            .grid_column_span(3)
                            .horizontal_alignment(HorizontalAlignment::Left)
                            .vertical_alignment(VerticalAlignment::Bottom)
                            .margin(Thickness::new(10.0, 0.0, 0.0, 8.0))
                            .padding(Thickness::new(6.0, 0.0, 6.0, 1.0))
                            .corner_radius(4.0)
                            .background(ThemeBrush::CardBackground)
                            .content(TextBlock::new().text("Preview").font_size(12.0)),
                    )),
            )
            .into()
    }
}
