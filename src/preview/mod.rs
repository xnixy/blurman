//! The Apps page's preview: Blurman's own frost brush (`glass::frost_graph`), drawn by WinUI's
//! compositor over what the window shows behind it, so the sliders show the real effect.
//!
//! The glass behind apps blurs the desktop through the system compositor. Here the same effect
//! graph runs on WinUI's compositor with an in-window backdrop: the sample shapes the page draws
//! under the preview's pane. Acrylic is Windows' own glass and has no graph; it is shown as frost
//! at acrylic's strength, milky by its slider.

use crate::glass::{self, FrostValue};
use crate::mapping::{self, BlurStyle, Look};
use winui_composition::windows_core::{self as winui, Interface};
use winui_composition::*;

/// A failure here, in the windows-rs Reactor uses (the preview is drawn with its bindings).
type Result<T> = winui::Result<T>;

/// A frost brush on a sprite that fills the element WinUI hosts it in, with rounded corners.
pub struct Preview {
    _compositor: Compositor,
    brush: CompositionEffectBrush,
    visual: SpriteVisual,
    corners: CompositionRoundedRectangleGeometry,
}

impl Preview {
    /// Build the preview for the compositor of the element it will sit in.
    pub fn new(host: winui::IUnknown) -> Result<Self> {
        let compositor: Compositor = host.cast()?;
        // The effect graph is built with the windows 0.61 crates the engine uses; its two inputs
        // are WinUI source parameters. The objects cross between the two bindings as COM pointers.
        let graph = glass::frost_graph(&|name| {
            let parameter = CompositionEffectSourceParameter::Create(name).map_err(to_engine)?;
            let unknown: winui::IUnknown = parameter.cast().map_err(to_engine)?;
            // SAFETY: an owned COM reference moves from one binding to the other.
            let engine: windows::core::IUnknown =
                unsafe { <windows::core::IUnknown as windows::core::Interface>::from_raw(unknown.into_raw()) };
            windows::core::Interface::cast(&engine)
        })
        .map_err(from_engine)?;
        let effect: windows::Graphics::Effects::IGraphicsEffect =
            windows::core::Interface::cast(&graph).map_err(from_engine)?;
        // SAFETY: as above.
        let effect = unsafe { IGraphicsEffect::from_raw(windows::core::Interface::into_raw(effect)) };
        let names = winui_composition::windows_collections::IIterable::<winui::HSTRING>::from(
            glass::ANIMATABLE
                .into_iter()
                .map(winui::HSTRING::from)
                .collect::<Vec<_>>(),
        );
        let factory = compositor.CreateEffectFactoryWithProperties(&effect, &names)?;
        let brush = factory.CreateBrush()?;
        brush.SetSourceParameter(
            "backdrop",
            &compositor.cast::<ICompositor2>()?.CreateBackdropBrush()?,
        )?;
        // The preview has no grain tile; grain shows on the real glass.
        brush.SetSourceParameter("noise", &compositor.CreateColorBrush()?)?;
        let visual = compositor.CreateSpriteVisual()?;
        visual
            .cast::<IVisual2>()?
            .SetRelativeSizeAdjustment(winui_composition::windows_numerics::Vector2 { x: 1.0, y: 1.0 })?;
        visual.SetBrush(&brush)?;
        let corners = compositor
            .cast::<ICompositor5>()?
            .CreateRoundedRectangleGeometry()?;
        corners.SetCornerRadius(winui_composition::windows_numerics::Vector2 { x: 8.0, y: 8.0 })?;
        let clip = compositor
            .cast::<ICompositor6>()?
            .CreateGeometricClipWithGeometry(&corners)?;
        visual.cast::<IVisual>()?.SetClip(&clip)?;
        Ok(Self {
            _compositor: compositor,
            brush,
            visual,
            corners,
        })
    }

    /// The visual to give the hosting element.
    pub fn visual(&self) -> winui::IUnknown {
        self.visual.cast().expect("a sprite visual is an IUnknown")
    }

    /// The hosting element's size, in its own pixels, for the rounded corners.
    pub fn resize(&self, width: f64, height: f64) {
        let _ = self.corners.SetSize(winui_composition::windows_numerics::Vector2 {
            x: width as f32,
            y: height as f32,
        });
    }

    /// Show `look`.
    pub fn set_look(&self, look: &Look) -> Result<()> {
        let mut finish = look.finish;
        let radius = match look.style {
            BlurStyle::Frost => mapping::blur_strength_to_radius(look.blur),
            // Acrylic's slider sets how milky it is.
            BlurStyle::Acrylic => {
                finish.milk = finish.milk.max((look.blur as f32 * 0.7) as u8);
                mapping::blur_strength_to_radius(mapping::BLUR_MAX) * 0.6
            }
        };
        let props = self.brush.cast::<ICompositionObject>()?.Properties()?;
        props.InsertScalar(glass::BLUR_PROPERTY, radius)?;
        for (name, value) in glass::finish_values(&finish, glass::tint_now(&finish)) {
            match value {
                FrostValue::Scalar(value) => props.InsertScalar(name, value)?,
                FrostValue::Color([a, r, g, b]) => {
                    props.InsertColor(name, Color { a, r, g, b })?
                }
            }
        }
        Ok(())
    }
}

fn to_engine(err: winui::Error) -> windows::core::Error {
    windows::core::Error::from_hresult(windows::core::HRESULT(err.code().0))
}

fn from_engine(err: windows::core::Error) -> winui::Error {
    winui::Error::from_hresult(winui::HRESULT(err.code().0))
}
