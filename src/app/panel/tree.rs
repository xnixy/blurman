//! The composition visuals of a backdrop window: one or more glass pieces, each from the bottom
//! up the shadow, the blurred backdrop (through the style's luminosity, as acrylic does), the
//! tint, and the rim. A piece's backdrop and tint are clipped to one rounded rectangle; its
//! shadow and rim are shapes of the same rectangle. Everything is in pixels.
//!
//! Pieces are made the first time that many are asked for and then kept: a piece no longer
//! asked for is only hidden, and a piece whose place is unchanged is not touched, so a window
//! whose islands are laid out every frame costs nothing while they stay put.

use super::effect;
use super::guard;
use super::style::{Luminosity, Style};
use std::cell::{Cell, OnceCell, RefCell};
use windows::core::{Interface, Result};
use windows::System::{DispatcherQueue, DispatcherQueueController};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::WinRT::Composition::ICompositorDesktopInterop;
use windows::Win32::System::WinRT::{
    CreateDispatcherQueueController, DispatcherQueueOptions, DQTAT_COM_NONE, DQTYPE_THREAD_CURRENT,
};
use windows::UI::Color;
use windows::UI::Composition::Desktop::DesktopWindowTarget;
use windows::UI::Composition::{
    CompositionBackdropBrush, CompositionColorBrush, CompositionColorGradientStop,
    CompositionEffectBrush, CompositionLinearGradientBrush, CompositionRoundedRectangleGeometry,
    CompositionVisualSurface, Compositor, ContainerVisual, DropShadow, ShapeVisual, SpriteVisual,
    Visual,
};
use windows_numerics::{Vector2, Vector3};

thread_local! {
    /// One compositor per UI thread, with the dispatcher queue it needs to commit its changes
    /// (the window's own message loop runs that queue). None once it has failed.
    static COMPOSITOR: OnceCell<Option<(Compositor, Option<DispatcherQueueController>)>> =
        const { OnceCell::new() };
}

/// This thread's compositor, made on first use.
pub fn compositor() -> Option<Compositor> {
    COMPOSITOR.with(|cell| {
        cell.get_or_init(|| {
            let queue = match DispatcherQueue::GetForCurrentThread() {
                Ok(_) => None,
                Err(_) => {
                    let options = DispatcherQueueOptions {
                        dwSize: size_of::<DispatcherQueueOptions>() as u32,
                        threadType: DQTYPE_THREAD_CURRENT,
                        apartmentType: DQTAT_COM_NONE,
                    };
                    Some(unsafe { CreateDispatcherQueueController(options) }.ok()?)
                }
            };
            Some((Compositor::new().ok()?, queue))
        })
        .as_ref()
        .map(|(c, _)| c.clone())
    })
}

fn color(c: egui::Color32) -> Color {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    Color {
        A: a,
        R: r,
        G: g,
        B: b,
    }
}

fn v2(x: f32, y: f32) -> Vector2 {
    Vector2 { X: x, Y: y }
}

fn v3(x: f32, y: f32) -> Vector3 {
    Vector3 { X: x, Y: y, Z: 0.0 }
}

/// Where one piece goes, in the backdrop window's pixels.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Placed {
    pub left: i32,
    pub top: i32,
    pub size: (i32, i32),
    /// Corner radius in pixels, already fitted to the size.
    pub radius: f32,
    /// How much of the piece shows, 0 to 1: less lets the unblurred desktop through.
    pub opacity: f32,
    /// Whether it casts the style's shadow.
    pub shadow: bool,
}

/// What the pieces are coloured for: the look, whether transparency effects are on, and the
/// shadow's blur and offset in pixels.
#[derive(Clone, Copy, PartialEq)]
pub struct Paint {
    pub style: Style,
    pub effects: bool,
    pub shadow_blur: f32,
    pub shadow_offset: f32,
}

pub struct Tree {
    /// Keeps the visuals on the window.
    _target: DesktopWindowTarget,
    root: ContainerVisual,
    compositor: Compositor,
    host_backdrop: CompositionBackdropBrush,
    /// The host backdrop through the style's luminosity, made again only when that changes. None
    /// where composition would not build the effect: then the blur shows as it is.
    lifted: RefCell<Option<(Luminosity, Option<CompositionEffectBrush>)>>,
    /// The backdrop when transparency effects are off.
    solid: CompositionColorBrush,
    /// What every piece shares: the tint, the filled shape under the glass, and the rim's line.
    tint: CompositionColorBrush,
    mask_fill: CompositionColorBrush,
    rim: CompositionLinearGradientBrush,
    rim_stops: [CompositionColorGradientStop; 3],
    pieces: RefCell<Vec<Piece>>,
    /// What the shared brushes and every piece were last coloured for.
    painted: Cell<Option<Paint>>,
}

/// One rounded piece of glass.
struct Piece {
    /// Holds the rest, at the piece's corner; its opacity is the piece's.
    group: ContainerVisual,
    /// The shadow's caster, the clipped body and the rim.
    layers: [Visual; 3],
    caster: SpriteVisual,
    shadow: DropShadow,
    /// The filled shape, drawn into a surface that both paints the caster and masks its shadow.
    mask_visual: ShapeVisual,
    mask: CompositionVisualSurface,
    backdrop: SpriteVisual,
    tint_visual: SpriteVisual,
    rim_visual: ShapeVisual,
    /// The clip, the shadow's shape, and the rim's (half a pixel in, so its line is crisp).
    shapes: [CompositionRoundedRectangleGeometry; 3],
    placed: Cell<Option<Placed>>,
    shown: Cell<bool>,
}

impl Tree {
    /// The visuals for the backdrop window `hwnd`, drawn as its only content.
    pub fn new(compositor: &Compositor, hwnd: HWND) -> Result<Self> {
        let c = compositor;
        let interop: ICompositorDesktopInterop = c.cast()?;
        let target = unsafe { interop.CreateDesktopWindowTarget(hwnd, true)? };
        let root = c.CreateContainerVisual()?;
        target.SetRoot(&root)?;

        // The rim: brightest along the top, fading to a faint line by a third of the way down.
        let rim = c.CreateLinearGradientBrush()?;
        rim.SetStartPoint(v2(0.0, 0.0))?;
        rim.SetEndPoint(v2(0.0, 1.0))?;
        let rim_stops = [
            c.CreateColorGradientStop()?,
            c.CreateColorGradientStop()?,
            c.CreateColorGradientStop()?,
        ];
        for (stop, offset) in rim_stops.iter().zip([0.0, 0.3, 1.0]) {
            stop.SetOffset(offset)?;
            rim.ColorStops()?.Append(stop)?;
        }
        Ok(Self {
            _target: target,
            root,
            compositor: c.clone(),
            host_backdrop: c.CreateHostBackdropBrush()?,
            lifted: RefCell::new(None),
            solid: c.CreateColorBrush()?,
            tint: c.CreateColorBrush()?,
            mask_fill: c.CreateColorBrush()?,
            rim,
            rim_stops,
            pieces: RefCell::new(Vec::new()),
            painted: Cell::new(None),
        })
    }

    /// Show one piece of glass at each of `placed`, coloured for `paint`, and hide the rest.
    /// Only what changed is touched.
    ///
    /// Everything goes through [`guard`] first: a piece of no size is hidden, never sized to 0.
    pub fn apply(&self, placed: &[Placed], paint: Paint) -> Result<()> {
        let paint = guard::paint(paint);
        let placed: Vec<Placed> = placed.iter().filter_map(guard::piece).collect();
        let repaint = self.painted.get() != Some(paint);
        if repaint {
            self.paint_shared(&paint)?;
        }
        let mut pieces = self.pieces.borrow_mut();
        while pieces.len() < placed.len() {
            let piece = Piece::new(self)?;
            piece.paint(self, &paint)?;
            pieces.push(piece);
        }
        for (i, piece) in pieces.iter().enumerate() {
            match placed.get(i) {
                Some(p) => {
                    if repaint {
                        piece.paint(self, &paint)?;
                    }
                    piece.place(p)?;
                    piece.show(true)?;
                }
                None => piece.show(false)?,
            }
        }
        self.painted.set(Some(paint));
        Ok(())
    }

    /// Colour what the pieces share.
    fn paint_shared(&self, paint: &Paint) -> Result<()> {
        let style = &paint.style;
        self.solid.SetColor(color(style.solid))?;
        self.tint.SetColor(color(style.tint))?;
        // The filled shape under the glass: the tint's colour, so its edge cannot darken the
        // glass's own anti-aliased edge.
        self.mask_fill.SetColor(color(style.tint.to_opaque()))?;
        let [rr, rg, rb, ra] = style.rim.to_srgba_unmultiplied();
        let faint = Color {
            A: ra / 3,
            R: rr,
            G: rg,
            B: rb,
        };
        for (stop, c) in self.rim_stops.iter().zip([color(style.rim), faint, faint]) {
            stop.SetColor(c)?;
        }
        if paint.effects {
            let mut lifted = self.lifted.borrow_mut();
            if lifted.as_ref().is_none_or(|(l, _)| *l != style.luminosity) {
                let brush = effect::color_matrix_brush(
                    &self.compositor,
                    &self.host_backdrop.cast()?,
                    style.luminosity.matrix(),
                )
                .ok();
                *lifted = Some((style.luminosity, brush));
            }
        }
        Ok(())
    }
}

impl Piece {
    fn new(tree: &Tree) -> Result<Self> {
        let c = &tree.compositor;
        let shapes = [
            c.CreateRoundedRectangleGeometry()?,
            c.CreateRoundedRectangleGeometry()?,
            c.CreateRoundedRectangleGeometry()?,
        ];

        // The shadow: a sprite painted with the filled shape, which also masks its shadow. (A
        // layer visual's shadow follows the layer's rectangle, not its content: measured.) The
        // glass covers the fill, so only the shadow outside the shape shows.
        let mask_visual = c.CreateShapeVisual()?;
        let mask_shape = c.CreateSpriteShapeWithGeometry(&shapes[1])?;
        mask_shape.SetFillBrush(&tree.mask_fill)?;
        mask_visual.Shapes()?.Append(&mask_shape)?;
        let mask = c.CreateVisualSurface()?;
        mask.SetSourceVisual(&mask_visual)?;
        let mask_brush = c.CreateSurfaceBrushWithSurface(&mask)?;
        let shadow = c.CreateDropShadow()?;
        shadow.SetMask(&mask_brush)?;
        let caster = c.CreateSpriteVisual()?;
        caster.SetBrush(&mask_brush)?;
        caster.SetShadow(&shadow)?;

        let body = c.CreateContainerVisual()?;
        body.SetClip(&c.CreateGeometricClipWithGeometry(&shapes[0])?)?;
        let backdrop = c.CreateSpriteVisual()?;
        let tint_visual = c.CreateSpriteVisual()?;
        tint_visual.SetBrush(&tree.tint)?;
        body.Children()?.InsertAtTop(&backdrop)?;
        body.Children()?.InsertAtTop(&tint_visual)?;

        let rim_visual = c.CreateShapeVisual()?;
        let rim_shape = c.CreateSpriteShapeWithGeometry(&shapes[2])?;
        rim_shape.SetStrokeBrush(&tree.rim)?;
        rim_shape.SetStrokeThickness(1.0)?;
        rim_visual.Shapes()?.Append(&rim_shape)?;

        // Made hidden, the shadow too: nothing shows (and DWM sizes no texture for it) until
        // `place` has given every visual and the mask surface a real size.
        caster.SetIsVisible(false)?;
        let group = c.CreateContainerVisual()?;
        group.SetIsVisible(false)?;
        let children = group.Children()?;
        children.InsertAtTop(&caster)?;
        children.InsertAtTop(&body)?;
        children.InsertAtTop(&rim_visual)?;
        // Later pieces go over earlier ones, so the caller's order is the stacking order.
        tree.root.Children()?.InsertAtTop(&group)?;
        Ok(Self {
            group,
            layers: [caster.cast()?, body.cast()?, rim_visual.cast()?],
            caster,
            shadow,
            mask_visual,
            mask,
            backdrop,
            tint_visual,
            rim_visual,
            shapes,
            placed: Cell::new(None),
            shown: Cell::new(false),
        })
    }

    fn paint(&self, tree: &Tree, paint: &Paint) -> Result<()> {
        let style = &paint.style;
        let [sr, sg, sb, sa] = style.shadow.color.to_srgba_unmultiplied();
        self.shadow.SetColor(Color {
            A: 255,
            R: sr,
            G: sg,
            B: sb,
        })?;
        self.shadow.SetOpacity(sa as f32 / 255.0)?;
        self.shadow.SetBlurRadius(paint.shadow_blur)?;
        self.shadow.SetOffset(v3(0.0, paint.shadow_offset))?;
        if paint.effects {
            let lifted = tree.lifted.borrow();
            match lifted.as_ref().and_then(|(_, b)| b.as_ref()) {
                Some(brush) => self.backdrop.SetBrush(brush)?,
                None => self.backdrop.SetBrush(&tree.host_backdrop)?,
            }
        } else {
            self.backdrop.SetBrush(&tree.solid)?;
        }
        Ok(())
    }

    fn place(&self, p: &Placed) -> Result<()> {
        if self.placed.get() == Some(*p) {
            return Ok(());
        }
        // Only ever a piece `guard::piece` passed: every side at least 1 px.
        debug_assert!(p.size.0 >= 1 && p.size.1 >= 1);
        let before = self.placed.replace(Some(*p));
        self.group.SetOffset(v3(p.left as f32, p.top as f32))?;
        self.group.SetOpacity(p.opacity)?;
        if !before.is_some_and(|b| (b.size, b.radius) == (p.size, p.radius)) {
            self.resize(p)?;
        }
        // Without a shadow the caster goes too: its opaque fill would show through a piece that
        // is not fully opaque. Only after the sizes, so the shadow never shows on a mask of no
        // size.
        self.caster.SetIsVisible(p.shadow)?;
        Ok(())
    }

    fn resize(&self, p: &Placed) -> Result<()> {
        let size = v2(p.size.0 as f32, p.size.1 as f32);
        let r = p.radius;
        self.group.SetSize(size)?;
        for layer in &self.layers {
            layer.SetSize(size)?;
        }
        for visual in [&self.mask_visual, &self.rim_visual] {
            visual.SetSize(size)?;
        }
        self.mask.SetSourceSize(size)?;
        for visual in [&self.backdrop, &self.tint_visual] {
            visual.SetSize(size)?;
        }
        for shape in &self.shapes[..2] {
            shape.SetSize(size)?;
            shape.SetCornerRadius(v2(r, r))?;
        }
        let rim = &self.shapes[2];
        rim.SetOffset(v2(0.5, 0.5))?;
        rim.SetSize(v2((size.X - 1.0).max(0.0), (size.Y - 1.0).max(0.0)))?;
        rim.SetCornerRadius(v2((r - 0.5).max(0.0), (r - 0.5).max(0.0)))?;
        Ok(())
    }

    fn show(&self, on: bool) -> Result<()> {
        if self.shown.replace(on) != on {
            self.group.SetIsVisible(on)?;
        }
        Ok(())
    }
}
