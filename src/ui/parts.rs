//! The pieces every page is made of, after the design boards: WinUI's type ramp, cards with a
//! hairline, and Windows Settings-style rows (title and words on the left, the control on the
//! right).

use crate::target::AppGroup;
use windows_reactor::*;

/// WinUI's display face, for page titles and the selected app's name.
const DISPLAY_FONT: &str = "Segoe UI Variable Display";

/// A page: scrolls, with the boards' margins.
pub fn page_frame(content: impl Into<View>) -> View {
    ScrollViewer::new()
        .content(
            Border::new()
                .padding(Thickness::new(32.0, 4.0, 32.0, 28.0))
                .content(content),
        )
        .into()
}

/// A page's title and the line under it.
pub fn page_header(heading: &str, words: &str) -> StackPanel {
    StackPanel::new().children((title(heading), caption(words)))
}

/// A page title: WinUI's Title style.
pub fn title(text: &str) -> TextBlock {
    TextBlock::new()
        .text(text)
        .font_family(DISPLAY_FONT)
        .font_size(28.0)
        .font_weight(FontWeight::SEMI_BOLD)
}

/// WinUI's Subtitle style.
pub fn subtitle_text(text: &str) -> TextBlock {
    TextBlock::new()
        .text(text)
        .font_family(DISPLAY_FONT)
        .font_size(20.0)
        .font_weight(FontWeight::SEMI_BOLD)
        .text_trimming(TextTrimming::CharacterEllipsis)
}

/// WinUI's BodyStrong style: section headings.
pub fn strong(text: &str) -> TextBlock {
    TextBlock::new().text(text).font_weight(FontWeight::SEMI_BOLD)
}

/// WinUI's Caption style in the secondary text color.
pub fn caption(text: &str) -> TextBlock {
    TextBlock::new()
        .text(text)
        .font_size(12.0)
        .foreground(ThemeBrush::SecondaryText)
        .text_wrapping(TextWrapping::Wrap)
}

/// Words in the caution color, for a problem a row has.
pub fn warning(text: &str) -> TextBlock {
    TextBlock::new()
        .text(text)
        .font_size(12.0)
        .foreground(ThemeBrush::SystemCaution)
        .text_wrapping(TextWrapping::Wrap)
}

pub fn glyph(code: &str, size: f64) -> FontIcon {
    FontIcon::new().glyph(code).font_size(size)
}

/// A button's words with an icon in front.
pub fn with_glyph(code: &str, text: &str) -> View {
    StackPanel::new()
        .orientation(Orientation::Horizontal)
        .spacing(8.0)
        .children((
            glyph(code, 14.0).vertical_alignment(VerticalAlignment::Center),
            TextBlock::new().text(text),
        ))
        .into()
}

/// A card: the boards' fill, hairline and corners.
pub fn card(padding: Thickness, content: impl Into<View>) -> Border {
    Border::new()
        .background(ThemeBrush::CardBackground)
        .border_brush(ThemeBrush::CardStroke)
        .border_thickness(1.0)
        .corner_radius(7.0)
        .padding(padding)
        .content(content)
}

/// A heading over a card of rows.
pub fn section(heading: &str, items: Vec<View>) -> View {
    StackPanel::new()
        .spacing(8.0)
        .children((
            strong(heading),
            card(Thickness::new(0.0, 0.0, 0.0, 0.0), rows(items)),
        ))
        .into()
}

/// Rows, with a hairline between each.
pub fn rows(items: Vec<View>) -> View {
    let mut children: Vec<View> = Vec::new();
    for (index, item) in items.into_iter().enumerate() {
        if index > 0 {
            children.push(
                Border::new()
                    .height(1.0)
                    .background(ThemeBrush::CardStroke)
                    .into(),
            );
        }
        children.push(item);
    }
    StackPanel::new().children(children).into()
}

/// A row: its title and what it says under that on the left, its control on the right.
pub fn row(heading: &str, words: &str, control: impl Into<View>) -> View {
    row_with(heading, words, Border::new().into(), control.into())
}

/// A row with something more under its words (title chips, a problem).
pub fn row_with(heading: &str, words: &str, under: View, control: View) -> View {
    Border::new()
        .padding(Thickness::new(16.0, 10.0, 16.0, 10.0))
        .min_height(62.0)
        .content(
            Grid::new()
                .columns([GridLength::STAR, GridLength::Auto])
                .column_spacing(16.0)
                .children((
                    StackPanel::new()
                        .vertical_alignment(VerticalAlignment::Center)
                        .children((
                            TextBlock::new()
                                .text(heading)
                                .text_wrapping(TextWrapping::Wrap),
                            caption(words),
                            under,
                        )),
                    Border::new()
                        .grid_column(1)
                        .vertical_alignment(VerticalAlignment::Center)
                        .content(control),
                )),
        )
        .into()
}

/// A switch without WinUI's On and Off words beside it: the row says what it is.
pub fn switch(on: bool, label: &str, toggled: Callback<bool>) -> View {
    ToggleSwitch::new()
        .is_on(on)
        .on_content("")
        .off_content("")
        .min_width(0.0)
        .automation_name(label)
        .on_toggled(toggled)
        .into()
}

/// A slider with its value written after it.
pub fn slider(value: u8, min: u8, max: u8, unit: &str, label: &str, changed: Callback<f64>) -> View {
    slider_sized(value, min, max, unit, label, 200.0, changed)
}

pub fn slider_sized(
    value: u8,
    min: u8,
    max: u8,
    unit: &str,
    label: &str,
    width: f64,
    changed: Callback<f64>,
) -> View {
    StackPanel::new()
        .orientation(Orientation::Horizontal)
        .spacing(12.0)
        .children((
            Slider::new()
                .width(width)
                .minimum(min as f64)
                .maximum(max as f64)
                .step_frequency(1.0)
                .value(value as f64)
                .automation_name(label)
                .vertical_alignment(VerticalAlignment::Center)
                .on_value_changed(changed),
            TextBlock::new()
                .text(format!("{value}{unit}"))
                .width(40.0)
                .text_alignment(TextAlignment::Right)
                .foreground(ThemeBrush::SecondaryText)
                .vertical_alignment(VerticalAlignment::Center),
        ))
        .into()
}

/// The exe's own icon, or a plain tile where it has none.
pub fn app_icon(png: Option<EncodedImage>, size: f64) -> Border {
    match png {
        Some(png) => Border::new()
            .width(size)
            .height(size)
            .content(Image::new().source_data(png).width(size).height(size)),
        None => Border::new()
            .width(size)
            .height(size)
            .corner_radius(size / 4.0)
            .background(ThemeBrush::SubtleSecondary)
            .content(
                glyph("\u{E71D}", size / 2.0)
                    .foreground(ThemeBrush::SecondaryText)
                    .horizontal_alignment(HorizontalAlignment::Center)
                    .vertical_alignment(VerticalAlignment::Center),
            ),
    }
}

/// The badge on an app that runs as administrator.
pub fn admin_tag() -> Border {
    Border::new()
        .border_brush(ThemeBrush::SystemCaution)
        .border_thickness(1.0)
        .corner_radius(4.0)
        .padding(Thickness::new(6.0, 0.0, 6.0, 1.0))
        .vertical_alignment(VerticalAlignment::Center)
        .content(
            TextBlock::new()
                .text("Admin")
                .font_size(11.0)
                .foreground(ThemeBrush::SystemCaution),
        )
}

/// A slider's value as one of the look's whole numbers, in its range.
pub fn clamp(value: f64, min: u8, max: u8) -> u8 {
    value.round().clamp(min as f64, max as f64) as u8
}

/// What a list row says under an app's name: its windows, or that it is not running. With
/// BLURMAN_UI_REDACT (debug builds, for screenshots), window titles are left out.
pub fn subtitle(group: Option<&AppGroup>) -> String {
    let redact = cfg!(debug_assertions) && std::env::var_os("BLURMAN_UI_REDACT").is_some();
    match group {
        Some(group) if redact && group.windows > 1 => format!("{} windows", group.windows),
        Some(_) if redact => "1 window".to_string(),
        Some(group) if group.windows > 1 => {
            format!("{} windows · {}", group.windows, group.sample_title)
        }
        Some(group) => group.sample_title.clone(),
        None => "Not running".to_string(),
    }
}

/// The exe's icon as a 48 px PNG, for an Image.
pub fn icon_png(path: &str) -> Option<EncodedImage> {
    let icon = crate::icons::exe_icon(path, 48)?;
    let pixels = image::RgbaImage::from_raw(icon.size, icon.size, icon.rgba)?;
    let mut bytes = Vec::new();
    pixels
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .ok()?;
    Some(EncodedImage::new(bytes))
}
