//! A glass window that sits behind another app and blurs whatever is behind it.

use crate::mapping::{self, BlurStyle, Finish, Look, Rgb, TintSource};
use crate::system::{self, SystemColors};
use std::time::Duration;
use windows::core::{implement, IInspectable, Interface, BOOL, GUID, HSTRING, PCWSTR};
use windows::Foundation::{IPropertyValue, PropertyValue, Size, TimeSpan};
use windows::Graphics::DirectX::{DirectXAlphaMode, DirectXPixelFormat};
use windows::Graphics::Effects::{
    IGraphicsEffect, IGraphicsEffectSource, IGraphicsEffectSource_Impl, IGraphicsEffect_Impl,
};
use windows::System::DispatcherQueueController;
use windows::Win32::Foundation::{
    E_INVALIDARG, HINSTANCE, HMODULE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT, D2D_RECT_F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    CLSID_D2D1Blend, CLSID_D2D1Border, CLSID_D2D1Composite, CLSID_D2D1Contrast, CLSID_D2D1Flood,
    CLSID_D2D1GaussianBlur, CLSID_D2D1Opacity, CLSID_D2D1Saturation, D2D1CreateFactory,
    ID2D1DeviceContext, ID2D1Factory1, D2D1_BITMAP_PROPERTIES1, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_TRANSITIONS_FORCEDISABLED, DWMWA_USE_HOSTBACKDROPBRUSH,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWINDOWATTRIBUTE,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows::Win32::System::WinRT::Composition::{
    ICompositionDrawingSurfaceInterop, ICompositorDesktopInterop, ICompositorInterop,
};
use windows::Win32::System::WinRT::Graphics::Direct2D::{
    IGraphicsEffectD2D1Interop, IGraphicsEffectD2D1Interop_Impl, GRAPHICS_EFFECT_PROPERTY_MAPPING,
    GRAPHICS_EFFECT_PROPERTY_MAPPING_COLOR_TO_VECTOR4, GRAPHICS_EFFECT_PROPERTY_MAPPING_DIRECT,
};
use windows::Win32::System::WinRT::{
    CreateDispatcherQueueController, DispatcherQueueOptions, RoInitialize, DQTAT_COM_STA,
    DQTYPE_THREAD_CURRENT, RO_INIT_SINGLETHREADED,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BeginDeferWindowPos, CreateWindowExW, DefWindowProcW, DeferWindowPos, DestroyWindow,
    EndDeferWindowPos, GetWindow, GetWindowRect, IsWindowVisible, RegisterClassExW, SetWindowPos,
    ShowWindow, GW_HWNDNEXT, HTTRANSPARENT, HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOACTIVATE,
    SWP_NOCOPYBITS, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOREDRAW, SWP_NOSIZE, SWP_SHOWWINDOW,
    SW_HIDE, WM_ERASEBKGND, WM_NCHITTEST, WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP,
    WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::UI::Color;
use windows::UI::Composition::Desktop::DesktopWindowTarget;
use windows::UI::Composition::{
    CompositionEffectBrush, CompositionEffectFactory, CompositionEffectSourceParameter,
    CompositionGraphicsDevice, CompositionPropertySet, CompositionStretch, CompositionSurfaceBrush,
    Compositor, ContainerVisual,
};
use windows_numerics::Vector2;

const WCA_ACCENT_POLICY: i32 = 19;
const ACCENT_DISABLED: i32 = 0;
const ACCENT_ENABLE_ACRYLICBLURBEHIND: i32 = 4;
const GLASS_CLASS: PCWSTR = windows::core::w!("BlurmanGlass");

/// Brush properties the window changes live, as "effect name.property name".
const BLUR_AMOUNT: &str = "Blur.BlurAmount";
const SATURATION: &str = "Saturation.Saturation";
const CONTRAST: &str = "Contrast.Contrast";
const MILK_COLOR: &str = "Milk.Color";
const TINT_COLOR: &str = "Tint.Color";
const GRAIN_OPACITY: &str = "Grain.Opacity";

/// The lightness milk lifts the glass toward. Just short of white, so the brightest colors
/// keep a little of their hue instead of clipping to white.
const MILK: Rgb = Rgb(0xf2, 0xf2, 0xf2);

/// How long new glass takes to come into focus.
const FADE_IN: Duration = Duration::from_millis(220);
/// Side of the grain tile, in pixels. It repeats across the pane.
const GRAIN_TILE: u32 = 128;

type Mapping = (&'static str, u32, GRAPHICS_EFFECT_PROPERTY_MAPPING);

const BLUR_NAMES: &[Mapping] = &[
    ("BlurAmount", 0, GRAPHICS_EFFECT_PROPERTY_MAPPING_DIRECT),
    (
        "StandardDeviation",
        0,
        GRAPHICS_EFFECT_PROPERTY_MAPPING_DIRECT,
    ),
];
const SATURATION_NAMES: &[Mapping] = &[("Saturation", 0, GRAPHICS_EFFECT_PROPERTY_MAPPING_DIRECT)];
const CONTRAST_NAMES: &[Mapping] = &[("Contrast", 0, GRAPHICS_EFFECT_PROPERTY_MAPPING_DIRECT)];
const FLOOD_NAMES: &[Mapping] = &[(
    "Color",
    0,
    GRAPHICS_EFFECT_PROPERTY_MAPPING_COLOR_TO_VECTOR4,
)];
const OPACITY_NAMES: &[Mapping] = &[("Opacity", 0, GRAPHICS_EFFECT_PROPERTY_MAPPING_DIRECT)];

/// One Direct2D effect in the glass graph, described the way composition reads it.
#[implement(IGraphicsEffect, IGraphicsEffectSource, IGraphicsEffectD2D1Interop)]
struct Effect {
    name: HSTRING,
    clsid: GUID,
    /// Properties composition may set by name, with their D2D index and how values convert.
    names: &'static [Mapping],
    properties: Vec<IPropertyValue>,
    sources: Vec<IGraphicsEffectSource>,
}

impl Effect {
    fn build(
        name: &str,
        clsid: GUID,
        names: &'static [Mapping],
        properties: Vec<IInspectable>,
        sources: Vec<IGraphicsEffectSource>,
    ) -> windows::core::Result<IGraphicsEffectSource> {
        let properties = properties
            .into_iter()
            .map(|value| value.cast())
            .collect::<windows::core::Result<Vec<IPropertyValue>>>()?;
        let effect: IGraphicsEffect = Effect {
            name: HSTRING::from(name),
            clsid,
            names,
            properties,
            sources,
        }
        .into();
        effect.cast()
    }
}

impl IGraphicsEffect_Impl for Effect_Impl {
    fn Name(&self) -> windows::core::Result<HSTRING> {
        Ok(self.name.clone())
    }

    fn SetName(&self, _value: &HSTRING) -> windows::core::Result<()> {
        Ok(())
    }
}

impl IGraphicsEffectSource_Impl for Effect_Impl {}

impl IGraphicsEffectD2D1Interop_Impl for Effect_Impl {
    fn GetEffectId(&self) -> windows::core::Result<GUID> {
        Ok(self.clsid)
    }

    fn GetNamedPropertyMapping(
        &self,
        name: &PCWSTR,
        index: *mut u32,
        mapping: *mut GRAPHICS_EFFECT_PROPERTY_MAPPING,
    ) -> windows::core::Result<()> {
        let text = unsafe { name.to_string() }.unwrap_or_default();
        let Some(&(_, property, how)) = self.names.iter().find(|(known, ..)| *known == text) else {
            return Err(E_INVALIDARG.into());
        };
        unsafe {
            if !index.is_null() {
                *index = property;
            }
            if !mapping.is_null() {
                *mapping = how;
            }
        }
        Ok(())
    }

    fn GetPropertyCount(&self) -> windows::core::Result<u32> {
        Ok(self.properties.len() as u32)
    }

    fn GetProperty(&self, index: u32) -> windows::core::Result<IPropertyValue> {
        self.properties
            .get(index as usize)
            .cloned()
            .ok_or_else(|| E_INVALIDARG.into())
    }

    fn GetSource(&self, index: u32) -> windows::core::Result<IGraphicsEffectSource> {
        self.sources
            .get(index as usize)
            .cloned()
            .ok_or_else(|| E_INVALIDARG.into())
    }

    fn GetSourceCount(&self) -> windows::core::Result<u32> {
        Ok(self.sources.len() as u32)
    }
}

struct Composition {
    _queue: DispatcherQueueController,
    compositor: Compositor,
    factory: CompositionEffectFactory,
    /// The tiled grain texture, or None where it could not be drawn. Grain is then left out.
    grain: Option<Grain>,
}

struct Grain {
    _device: CompositionGraphicsDevice,
    brush: CompositionSurfaceBrush,
}

/// Must be created and used on one thread that pumps messages.
pub struct GlassSession {
    composition: Option<Composition>,
    colors: SystemColors,
}

impl GlassSession {
    pub fn new() -> Self {
        let _ = unsafe { RoInitialize(RO_INIT_SINGLETHREADED) };
        let composition = match build_composition() {
            Ok(composition) => Some(composition),
            Err(err) => {
                eprintln!("Adjustable blur is unavailable ({err}). Using system acrylic.");
                None
            }
        };
        Self {
            composition,
            colors: system::system_colors(),
        }
    }

    /// True when the blur radius cannot be changed and the slider sets the tint instead.
    pub fn fallback(&self) -> bool {
        self.composition.is_none()
    }

    /// Read the Windows theme and accent again. True when either changed.
    pub fn refresh_colors(&mut self) -> bool {
        let colors = system::system_colors();
        let changed = colors != self.colors;
        self.colors = colors;
        changed
    }

    fn tint_color(&self, finish: &Finish) -> Rgb {
        match finish.tint_source {
            TintSource::Custom => finish.tint,
            TintSource::Theme => self.colors.theme_tint(),
            TintSource::Accent => self.colors.accent,
        }
    }

    pub fn open(&mut self, bounds: RECT, look: &Look) -> Result<GlassPane, String> {
        register_class()?;
        let hwnd = create_glass_window(bounds)?;
        let mut pane = GlassPane {
            hwnd,
            visuals: None,
            topmost: None,
            placed: None,
            corner: None,
            acrylic: None,
        };
        if let Err(err) = pane.set_look(self, look) {
            pane.close();
            return Err(err);
        }
        Ok(pane)
    }
}

struct Visuals {
    _target: DesktopWindowTarget,
    _root: ContainerVisual,
    brush: CompositionEffectBrush,
}

pub struct GlassPane {
    hwnd: HWND,
    visuals: Option<Visuals>,
    /// Which z-order band the pane is in, or None while it is hidden.
    topmost: Option<bool>,
    /// Bounds last given to SetWindowPos. Used to skip a DWM query when nothing moved.
    placed: Option<RECT>,
    /// Corner preference already applied, so a still pane is not written again.
    corner: Option<i32>,
    /// Milkiness and color while system acrylic is the effect. None while frost is attached.
    acrylic: Option<(u8, Rgb)>,
}

impl GlassPane {
    /// Draw `look`. Frost uses the adjustable blur when this PC has it, and system acrylic
    /// otherwise. Acrylic always uses the milky system glass, colored with the tint.
    pub fn set_look(&mut self, session: &mut GlassSession, look: &Look) -> Result<(), String> {
        let tint = session.tint_color(&look.finish);
        let frost_available = self.visuals.is_some() || session.composition.is_some();
        if look.style == BlurStyle::Acrylic || !frost_available {
            self.use_acrylic(look.blur, tint)
        } else {
            self.use_frost(session, look, tint)
        }
    }

    fn use_frost(
        &mut self,
        session: &mut GlassSession,
        look: &Look,
        tint: Rgb,
    ) -> Result<(), String> {
        if let Some(visuals) = &self.visuals {
            self.acrylic = None;
            return set_frost(visuals, look, tint).map_err(|err| err.to_string());
        }
        let attached = session.composition.as_ref().map(|composition| {
            let _ = clear_acrylic(self.hwnd);
            attach_frost(composition, self.hwnd, look, tint)
        });
        match attached {
            Some(Ok(visuals)) => {
                self.visuals = Some(visuals);
                self.acrylic = None;
                Ok(())
            }
            Some(Err(err)) => {
                eprintln!("Adjustable blur failed ({err}). Using system acrylic.");
                session.composition = None;
                self.use_acrylic(look.blur, tint)
            }
            None => self.use_acrylic(look.blur, tint),
        }
    }

    fn use_acrylic(&mut self, blur: u8, tint: Rgb) -> Result<(), String> {
        if self.visuals.take().is_some() {
            let _ = set_dwm_bool(self.hwnd, DWMWA_USE_HOSTBACKDROPBRUSH, false);
        }
        apply_acrylic(self.hwnd, blur, tint)?;
        self.acrylic = Some((blur, tint));
        Ok(())
    }

    /// Size the pane to `bounds` and put it directly beneath `target` in the z-order.
    /// Returns whether the pane was actually moved. A window that is already showing is not
    /// shown again: `SWP_SHOWWINDOW` replays the backdrop fade on every focus switch.
    pub fn place_under(&mut self, target: HWND, bounds: RECT, topmost: bool) -> bool {
        let moved = self.topmost != Some(topmost) || !self.sits_under(target, bounds);
        if moved {
            let visible = unsafe { IsWindowVisible(self.hwnd).as_bool() };
            let band = (topmost != self.topmost.unwrap_or(false)).then_some(if topmost {
                HWND_TOPMOST
            } else {
                HWND_NOTOPMOST
            });
            let mut flags = SWP_NOACTIVATE | SWP_NOCOPYBITS | SWP_NOOWNERZORDER;
            if !visible {
                flags |= SWP_SHOWWINDOW;
            }
            Self::commit_pos(
                self.hwnd,
                target,
                bounds.left,
                bounds.top,
                (bounds.right - bounds.left).max(1),
                (bounds.bottom - bounds.top).max(1),
                flags,
                band,
            );
            self.topmost = Some(topmost);
            if let Some((blur, tint)) = self.acrylic {
                let _ = apply_acrylic(self.hwnd, blur, tint);
            }
        }
        self.placed = Some(bounds);
        self.sync_corner(target);
        moved
    }

    /// Move the pane behind `target` without resizing it or asking DWM for the frame.
    /// Focus handling calls this first so the blur is in place before the slower follow-up.
    pub fn stack_under(&mut self, target: HWND) {
        if self.topmost.is_none() {
            return;
        }
        let topmost = crate::target::is_topmost(target);
        let band = (topmost != self.topmost.unwrap_or(false)).then_some(if topmost {
            HWND_TOPMOST
        } else {
            HWND_NOTOPMOST
        });
        if band.is_some() {
            self.topmost = Some(topmost);
        }
        Self::commit_pos(
            self.hwnd,
            target,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOCOPYBITS | SWP_NOOWNERZORDER,
            band,
        );
    }

    /// Copy the target's corner preference. Windows 10 has no preference, so a failed read leaves
    /// the pane on the system default instead of forcing a round corner.
    fn sync_corner(&mut self, target: HWND) {
        let Some(preference) = crate::target::corner_preference(target) else {
            return;
        };
        if self.corner == Some(preference) {
            return;
        }
        let value = preference;
        let ok = unsafe {
            DwmSetWindowAttribute(
                self.hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &value as *const i32 as *const std::ffi::c_void,
                std::mem::size_of::<i32>() as u32,
            )
            .is_ok()
        };
        if ok {
            self.corner = Some(preference);
        }
    }

    /// One z-order update. A band change and the move behind `insert` must land together.
    /// Separate calls paint a frame in between, and that frame is the old window on top.
    fn commit_pos(
        hwnd: HWND,
        insert: HWND,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        flags: windows::Win32::UI::WindowsAndMessaging::SET_WINDOW_POS_FLAGS,
        band: Option<HWND>,
    ) {
        let band_flags = SWP_NOMOVE
            | SWP_NOSIZE
            | SWP_NOACTIVATE
            | SWP_NOCOPYBITS
            | SWP_NOOWNERZORDER
            | SWP_NOREDRAW;
        unsafe {
            let count = if band.is_some() { 2 } else { 1 };
            let Ok(mut pending) = BeginDeferWindowPos(count) else {
                if let Some(band) = band {
                    let _ = SetWindowPos(hwnd, Some(band), 0, 0, 0, 0, band_flags);
                }
                let _ = SetWindowPos(hwnd, Some(insert), x, y, cx, cy, flags);
                return;
            };
            if let Some(band) = band {
                match DeferWindowPos(pending, hwnd, Some(band), 0, 0, 0, 0, band_flags) {
                    Ok(next) => pending = next,
                    Err(_) => return,
                }
            }
            if let Ok(pending) = DeferWindowPos(pending, hwnd, Some(insert), x, y, cx, cy, flags) {
                let _ = EndDeferWindowPos(pending);
            }
        }
    }

    /// Checks the real window rather than a cached position, so outside moves get corrected.
    fn sits_under(&self, target: HWND, bounds: RECT) -> bool {
        let mut rect = RECT::default();
        unsafe {
            GetWindowRect(self.hwnd, &mut rect).is_ok()
                && rect == bounds
                && IsWindowVisible(self.hwnd).as_bool()
                && GetWindow(target, GW_HWNDNEXT).ok() == Some(self.hwnd)
        }
    }

    pub fn hide(&mut self) {
        self.placed = None;
        if self.topmost.take().is_some() {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
        }
    }

    pub fn close(mut self) {
        self.visuals = None;
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

fn build_composition() -> windows::core::Result<Composition> {
    let queue = unsafe {
        CreateDispatcherQueueController(DispatcherQueueOptions {
            dwSize: std::mem::size_of::<DispatcherQueueOptions>() as u32,
            threadType: DQTYPE_THREAD_CURRENT,
            apartmentType: DQTAT_COM_STA,
        })?
    };
    let compositor = Compositor::new()?;
    compositor.CreateHostBackdropBrush()?;
    let effect: IGraphicsEffect = frost_graph()?.cast()?;
    let animatable = windows_collections::IIterable::<HSTRING>::from(
        [
            BLUR_AMOUNT,
            SATURATION,
            CONTRAST,
            MILK_COLOR,
            TINT_COLOR,
            GRAIN_OPACITY,
        ]
        .into_iter()
        .map(HSTRING::from)
        .collect::<Vec<_>>(),
    );
    let factory = compositor.CreateEffectFactoryWithProperties(&effect, &animatable)?;
    let grain = match grain_brush(&compositor) {
        Ok(grain) => Some(grain),
        Err(err) => {
            eprintln!("Grain is unavailable ({err}).");
            None
        }
    };
    Ok(Composition {
        _queue: queue,
        compositor,
        factory,
        grain,
    })
}

/// The backdrop, blurred, with its color strength and contrast changed, lifted toward milky white,
/// a tint laid over it, and grain on top.
/// Every stage is in the graph from the start, so a slider never has to rebuild the brush.
fn frost_graph() -> windows::core::Result<IGraphicsEffectSource> {
    let single = |value: f32| PropertyValue::CreateSingle(value);
    let uint = |value: u32| PropertyValue::CreateUInt32(value);
    let backdrop = CompositionEffectSourceParameter::Create(&HSTRING::from("backdrop"))?;
    let blur = Effect::build(
        "Blur",
        CLSID_D2D1GaussianBlur,
        BLUR_NAMES,
        // D2D1_GAUSSIANBLUR_OPTIMIZATION_BALANCED and D2D1_BORDER_MODE_HARD, passed as uint32.
        vec![
            single(mapping::blur_strength_to_radius(mapping::BLUR_DEFAULT))?,
            uint(1)?,
            uint(1)?,
        ],
        vec![backdrop.cast()?],
    )?;
    let saturation = Effect::build(
        "Saturation",
        CLSID_D2D1Saturation,
        SATURATION_NAMES,
        vec![single(1.0)?],
        vec![blur],
    )?;
    // D2D1_CONTRAST_PROP_CONTRAST, then CLAMP_INPUT so out-of-range values cannot bloom.
    let contrast = Effect::build(
        "Contrast",
        CLSID_D2D1Contrast,
        CONTRAST_NAMES,
        vec![single(0.0)?, PropertyValue::CreateBoolean(true)?],
        vec![saturation],
    )?;
    // Milk takes its lightness from a flood and keeps the glass's own hue and color strength,
    // the way Windows light acrylic builds its luminosity layer. A white wash would lighten and
    // grey the color together. The flood's alpha is how much.
    let milk = Effect::build(
        "Milk",
        CLSID_D2D1Flood,
        FLOOD_NAMES,
        vec![PropertyValue::CreateSingleArray(&[0.0, 0.0, 0.0, 0.0])?],
        Vec::new(),
    )?;
    // D2D1_BLEND_MODE_LUMINOSITY. The blend's first input is the source and the second the
    // destination, so the milk goes first: the glass keeps its color and takes the milk's
    // lightness. The other way round turns the glass grey.
    let milky = Effect::build(
        "Milky",
        CLSID_D2D1Blend,
        &[],
        vec![uint(23)?],
        vec![milk, contrast],
    )?;
    let tint = Effect::build(
        "Tint",
        CLSID_D2D1Flood,
        FLOOD_NAMES,
        vec![PropertyValue::CreateSingleArray(&[0.0, 0.0, 0.0, 0.0])?],
        Vec::new(),
    )?;
    // D2D1_COMPOSITE_MODE_SOURCE_OVER: the second source is drawn over the first.
    let tinted = Effect::build(
        "Tinted",
        CLSID_D2D1Composite,
        &[],
        vec![uint(0)?],
        vec![milky, tint],
    )?;
    let noise = CompositionEffectSourceParameter::Create(&HSTRING::from("noise"))?;
    // D2D1_BORDER_EDGE_MODE_WRAP on both axes, so the small tile covers any pane.
    let tiled = Effect::build(
        "Tile",
        CLSID_D2D1Border,
        &[],
        vec![uint(1)?, uint(1)?],
        vec![noise.cast()?],
    )?;
    let grain = Effect::build(
        "Grain",
        CLSID_D2D1Opacity,
        OPACITY_NAMES,
        vec![single(0.0)?],
        vec![tiled],
    )?;
    Effect::build(
        "Glass",
        CLSID_D2D1Composite,
        &[],
        vec![uint(0)?],
        vec![tinted, grain],
    )
}

/// Set every live property of an attached frost brush.
fn set_frost(visuals: &Visuals, look: &Look, tint: Rgb) -> windows::core::Result<()> {
    let props = visuals.brush.Properties()?;
    // A fade-in still running would override the new radius until it finished.
    let _ = visuals.brush.StopAnimation(&HSTRING::from(BLUR_AMOUNT));
    props.InsertScalar(
        &HSTRING::from(BLUR_AMOUNT),
        mapping::blur_strength_to_radius(look.blur),
    )?;
    set_finish(&props, &look.finish, tint)
}

fn set_finish(
    props: &CompositionPropertySet,
    finish: &Finish,
    tint: Rgb,
) -> windows::core::Result<()> {
    props.InsertScalar(&HSTRING::from(SATURATION), finish.saturation as f32 / 100.0)?;
    props.InsertScalar(&HSTRING::from(CONTRAST), finish.contrast as f32 / 100.0)?;
    props.InsertColor(
        &HSTRING::from(MILK_COLOR),
        Color {
            A: (finish.milk.min(mapping::MILK_MAX) as f32 * 2.55).round() as u8,
            R: MILK.0,
            G: MILK.1,
            B: MILK.2,
        },
    )?;
    let alpha = (finish.tint_amount.min(mapping::TINT_MAX) as f32 * 2.55).round() as u8;
    props.InsertColor(
        &HSTRING::from(TINT_COLOR),
        Color {
            A: alpha,
            R: tint.0,
            G: tint.1,
            B: tint.2,
        },
    )?;
    props.InsertScalar(&HSTRING::from(GRAIN_OPACITY), finish.grain as f32 / 100.0)
}

fn attach_frost(
    composition: &Composition,
    hwnd: HWND,
    look: &Look,
    tint: Rgb,
) -> windows::core::Result<Visuals> {
    set_dwm_bool(hwnd, DWMWA_USE_HOSTBACKDROPBRUSH, true)?;
    let compositor = &composition.compositor;
    let interop: ICompositorDesktopInterop = compositor.cast()?;
    let target = unsafe { interop.CreateDesktopWindowTarget(hwnd, false)? };
    let brush = composition.factory.CreateBrush()?;
    brush.SetSourceParameter(
        &HSTRING::from("backdrop"),
        &compositor.CreateHostBackdropBrush()?,
    )?;
    match &composition.grain {
        Some(grain) => brush.SetSourceParameter(&HSTRING::from("noise"), &grain.brush)?,
        None => brush.SetSourceParameter(
            &HSTRING::from("noise"),
            &compositor.CreateColorBrushWithColor(Color::default())?,
        )?,
    }
    let props = brush.Properties()?;
    let radius = mapping::blur_strength_to_radius(look.blur);
    props.InsertScalar(&HSTRING::from(BLUR_AMOUNT), radius)?;
    set_finish(&props, &look.finish, tint)?;
    // New glass comes into focus instead of snapping in. A failed animation leaves it sharp-edged
    // but correct, so it is not an error.
    let _ = fade_in(compositor, &brush, radius);

    let sprite = compositor.CreateSpriteVisual()?;
    sprite.SetRelativeSizeAdjustment(Vector2 { X: 1.0, Y: 1.0 })?;
    sprite.SetBrush(&brush)?;
    let root = compositor.CreateContainerVisual()?;
    root.SetRelativeSizeAdjustment(Vector2 { X: 1.0, Y: 1.0 })?;
    root.Children()?.InsertAtTop(&sprite)?;
    target.SetRoot(&root)?;
    Ok(Visuals {
        _target: target,
        _root: root,
        brush,
    })
}

/// Ease the blur from a quarter of its radius to all of it.
fn fade_in(
    compositor: &Compositor,
    brush: &CompositionEffectBrush,
    radius: f32,
) -> windows::core::Result<()> {
    let ease = compositor
        .CreateCubicBezierEasingFunction(Vector2 { X: 0.1, Y: 0.9 }, Vector2 { X: 0.2, Y: 1.0 })?;
    let animation = compositor.CreateScalarKeyFrameAnimation()?;
    animation.InsertKeyFrame(0.0, radius * 0.25)?;
    animation.InsertKeyFrameWithEasingFunction(1.0, radius, &ease)?;
    animation.SetDuration(TimeSpan {
        Duration: FADE_IN.as_nanos() as i64 / 100,
    })?;
    brush.StartAnimation(&HSTRING::from(BLUR_AMOUNT), &animation)
}

/// A tile of fine noise, drawn once with Direct2D into a composition surface.
fn grain_brush(compositor: &Compositor) -> windows::core::Result<Grain> {
    let d3d = d3d_device()?;
    let dxgi: IDXGIDevice = d3d.cast()?;
    let factory: ID2D1Factory1 =
        unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)? };
    let d2d = unsafe { factory.CreateDevice(&dxgi)? };
    let interop: ICompositorInterop = compositor.cast()?;
    let device = unsafe { interop.CreateGraphicsDevice(&d2d)? };
    let size = GRAIN_TILE as f32;
    let surface = device.CreateDrawingSurface(
        Size {
            Width: size,
            Height: size,
        },
        DirectXPixelFormat::B8G8R8A8UIntNormalized,
        DirectXAlphaMode::Premultiplied,
    )?;
    let drawing: ICompositionDrawingSurfaceInterop = surface.cast()?;
    let mut offset = POINT::default();
    let context: ID2D1DeviceContext = unsafe { drawing.BeginDraw(None, &mut offset)? };
    let drawn = unsafe { draw_grain(&context, offset) };
    unsafe { drawing.EndDraw()? };
    drawn?;
    let brush = compositor.CreateSurfaceBrushWithSurface(&surface)?;
    brush.SetStretch(CompositionStretch::None)?;
    Ok(Grain {
        _device: device,
        brush,
    })
}

fn d3d_device() -> windows::core::Result<ID3D11Device> {
    let mut last = None;
    for driver in [D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP] {
        let mut device = None;
        let created = unsafe {
            D3D11CreateDevice(
                None,
                driver,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                None,
            )
        };
        match (created, device) {
            (Ok(()), Some(device)) => return Ok(device),
            (Err(err), _) => last = Some(err),
            (Ok(()), None) => {}
        }
    }
    Err(last.unwrap_or_else(|| E_INVALIDARG.into()))
}

/// Light and dark specks at low alpha, so grain reads on dark and light backdrops alike.
unsafe fn draw_grain(context: &ID2D1DeviceContext, offset: POINT) -> windows::core::Result<()> {
    let mut pixels = Vec::with_capacity((GRAIN_TILE * GRAIN_TILE * 4) as usize);
    let mut seed = 0x2545_f491_u32;
    for _ in 0..GRAIN_TILE * GRAIN_TILE {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let shade = if seed & 1 == 0 { 255u32 } else { 0 };
        let alpha = (seed >> 8) % 56;
        let value = (shade * alpha / 255) as u8;
        pixels.extend_from_slice(&[value, value, value, alpha as u8]);
    }
    let format = D2D1_PIXEL_FORMAT {
        format: DXGI_FORMAT_B8G8R8A8_UNORM,
        alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
    };
    let bitmap = context.CreateBitmap(
        D2D_SIZE_U {
            width: GRAIN_TILE,
            height: GRAIN_TILE,
        },
        Some(pixels.as_ptr().cast()),
        GRAIN_TILE * 4,
        &D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: format,
            dpiX: 96.0,
            dpiY: 96.0,
            ..Default::default()
        },
    )?;
    context.SetDpi(96.0, 96.0);
    context.Clear(None);
    let left = offset.x as f32;
    let top = offset.y as f32;
    let size = GRAIN_TILE as f32;
    context.DrawBitmap(
        &bitmap,
        Some(&D2D_RECT_F {
            left,
            top,
            right: left + size,
            bottom: top + size,
        }),
        1.0,
        D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
        None,
        None,
    );
    Ok(())
}

fn set_dwm_bool(
    hwnd: HWND,
    attribute: DWMWINDOWATTRIBUTE,
    value: bool,
) -> windows::core::Result<()> {
    let value = BOOL::from(value);
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            attribute,
            &value as *const BOOL as *const std::ffi::c_void,
            std::mem::size_of::<BOOL>() as u32,
        )
    }
}

fn create_glass_window(bounds: RECT) -> Result<HWND, String> {
    let instance = unsafe { GetModuleHandleW(None) }.map_err(|err| err.to_string())?;
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT | WS_EX_NOREDIRECTIONBITMAP,
            GLASS_CLASS,
            PCWSTR::null(),
            WS_POPUP,
            bounds.left,
            bounds.top,
            (bounds.right - bounds.left).max(1),
            (bounds.bottom - bounds.top).max(1),
            None,
            None,
            Some(HINSTANCE(instance.0)),
            None,
        )
    }
    .map_err(|err| format!("Could not create the glass window: {err}"))?;
    // Without this, restacking the glass fades the blur in over the next frames.
    let _ = set_dwm_bool(hwnd, DWMWA_TRANSITIONS_FORCEDISABLED, true);
    Ok(hwnd)
}

fn register_class() -> Result<(), String> {
    static RESULT: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
    RESULT
        .get_or_init(|| {
            let instance = unsafe { GetModuleHandleW(None) }.map_err(|err| err.to_string())?;
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(glass_proc),
                hInstance: instance.into(),
                lpszClassName: GLASS_CLASS,
                ..Default::default()
            };
            if unsafe { RegisterClassExW(&class) } == 0 {
                return Err("Could not register the glass window class.".into());
            }
            Ok(())
        })
        .clone()
}

unsafe extern "system" fn glass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

#[repr(C)]
struct AccentPolicy {
    state: i32,
    flags: i32,
    color: u32,
    animation: i32,
}

#[repr(C)]
struct CompositionAttribute {
    attribute: i32,
    data: *mut AccentPolicy,
    size: usize,
}

type SetCompositionFn = unsafe extern "system" fn(HWND, *const CompositionAttribute) -> BOOL;

fn composition_fn() -> Option<SetCompositionFn> {
    static FN: std::sync::OnceLock<Option<SetCompositionFn>> = std::sync::OnceLock::new();
    *FN.get_or_init(|| unsafe {
        let lib = LoadLibraryW(windows::core::w!("user32.dll")).ok()?;
        let proc = GetProcAddress(lib, windows::core::s!("SetWindowCompositionAttribute"))?;
        Some(std::mem::transmute::<
            unsafe extern "system" fn() -> isize,
            SetCompositionFn,
        >(proc))
    })
}

/// Drop an acrylic policy so a frost brush is not stacked on top of it.
fn clear_acrylic(hwnd: HWND) -> Result<(), String> {
    apply_accent(hwnd, ACCENT_DISABLED, 0)
}

/// Undocumented acrylic blur-behind. It keeps rendering while the window is inactive.
fn apply_acrylic(hwnd: HWND, blur: u8, tint: Rgb) -> Result<(), String> {
    let alpha = mapping::blur_strength_to_tint_alpha(blur) as u32;
    // ABGR. The alpha byte is what the slider moves.
    let color = (alpha << 24) | (tint.2 as u32) << 16 | (tint.1 as u32) << 8 | tint.0 as u32;
    apply_accent(hwnd, ACCENT_ENABLE_ACRYLICBLURBEHIND, color)
}

fn apply_accent(hwnd: HWND, state: i32, color: u32) -> Result<(), String> {
    let set = composition_fn().ok_or("SetWindowCompositionAttribute is missing.")?;
    let mut policy = AccentPolicy {
        state,
        // Flag 2 tells acrylic to use the tint. A disabled policy leaves the glass alone.
        flags: if state == ACCENT_DISABLED { 0 } else { 2 },
        color,
        animation: 0,
    };
    let data = CompositionAttribute {
        attribute: WCA_ACCENT_POLICY,
        data: &mut policy,
        size: std::mem::size_of::<AccentPolicy>(),
    };
    if unsafe { set(hwnd, &data) }.as_bool() {
        Ok(())
    } else {
        Err("System acrylic was rejected.".into())
    }
}
