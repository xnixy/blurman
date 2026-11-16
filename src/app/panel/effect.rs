//! A Direct2D colour-matrix effect over the blurred backdrop, for composition. Composition takes
//! an effect as an `IGraphicsEffect` that also answers `IGraphicsEffectD2D1Interop` (what Win2D's
//! effect classes do); this is that, for the one effect the glass needs.
//!
//! The effect is built once per look: composition compiles it into a factory when the brush is
//! made, so there is nothing to run per frame.

use std::sync::Mutex;
use windows::core::{implement, Error, Interface, Result, GUID, HSTRING, PCWSTR};
use windows::Foundation::{IPropertyValue, PropertyValue};
use windows::Graphics::Effects::{
    IGraphicsEffect, IGraphicsEffectSource, IGraphicsEffectSource_Impl, IGraphicsEffect_Impl,
};
use windows::Win32::Foundation::E_INVALIDARG;
use windows::Win32::System::WinRT::Graphics::Direct2D::{
    IGraphicsEffectD2D1Interop, IGraphicsEffectD2D1Interop_Impl, GRAPHICS_EFFECT_PROPERTY_MAPPING,
};
use windows::UI::Composition::{
    CompositionBrush, CompositionEffectBrush, CompositionEffectSourceParameter, Compositor,
};

/// CLSID_D2D1ColorMatrix.
const COLOR_MATRIX: GUID = GUID::from_u128(0x921f03d6_641c_47df_852d_b4bb6153ae11);
/// D2D1_COLORMATRIX_ALPHA_MODE_PREMULTIPLIED.
const PREMULTIPLIED: u32 = 1;

/// The name the effect's one source goes by.
const SOURCE: &str = "backdrop";

#[implement(IGraphicsEffect, IGraphicsEffectSource, IGraphicsEffectD2D1Interop)]
struct ColorMatrix {
    name: Mutex<HSTRING>,
    /// D2D1_MATRIX_5X4_F, row by row: R, G, B and A in, then the offset; each row R, G, B, A out.
    matrix: [f32; 20],
    source: IGraphicsEffectSource,
}

impl IGraphicsEffectSource_Impl for ColorMatrix_Impl {}

impl IGraphicsEffect_Impl for ColorMatrix_Impl {
    fn Name(&self) -> Result<HSTRING> {
        Ok(self.name.lock().unwrap().clone())
    }

    fn SetName(&self, name: &HSTRING) -> Result<()> {
        *self.name.lock().unwrap() = name.clone();
        Ok(())
    }
}

impl IGraphicsEffectD2D1Interop_Impl for ColorMatrix_Impl {
    fn GetEffectId(&self) -> Result<GUID> {
        Ok(COLOR_MATRIX)
    }

    fn GetNamedPropertyMapping(
        &self,
        _name: &PCWSTR,
        _index: *mut u32,
        _mapping: *mut GRAPHICS_EFFECT_PROPERTY_MAPPING,
    ) -> Result<()> {
        // Nothing is animated.
        Err(Error::from(E_INVALIDARG))
    }

    fn GetPropertyCount(&self) -> Result<u32> {
        Ok(3)
    }

    fn GetProperty(&self, index: u32) -> Result<IPropertyValue> {
        match index {
            0 => PropertyValue::CreateSingleArray(&self.matrix)?.cast(),
            1 => PropertyValue::CreateUInt32(PREMULTIPLIED)?.cast(),
            // Clamp the output, so a lifted channel never goes past white.
            2 => PropertyValue::CreateBoolean(true)?.cast(),
            _ => Err(Error::from(E_INVALIDARG)),
        }
    }

    fn GetSource(&self, index: u32) -> Result<IGraphicsEffectSource> {
        if index == 0 {
            Ok(self.source.clone())
        } else {
            Err(Error::from(E_INVALIDARG))
        }
    }

    fn GetSourceCount(&self) -> Result<u32> {
        Ok(1)
    }
}

/// A brush that paints `source` through the colour `matrix` (see [`ColorMatrix::matrix`]).
pub fn color_matrix_brush(
    compositor: &Compositor,
    source: &CompositionBrush,
    matrix: [f32; 20],
) -> Result<CompositionEffectBrush> {
    let name = HSTRING::from(SOURCE);
    let effect: IGraphicsEffect = ColorMatrix {
        name: Mutex::new(HSTRING::new()),
        matrix,
        source: CompositionEffectSourceParameter::Create(&name)?.cast()?,
    }
    .into();
    let brush = compositor.CreateEffectFactory(&effect)?.CreateBrush()?;
    brush.SetSourceParameter(&name, source)?;
    Ok(brush)
}
