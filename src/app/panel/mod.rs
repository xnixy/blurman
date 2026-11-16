// Ported from shellman (edition 2021): its unsafe functions use unsafe calls directly, and it
// exports more than Blurman uses.
#![allow(unsafe_op_in_unsafe_fn, unused_imports, dead_code)]

//! Floating glass: a rounded surface of any radius with real blur, a soft shadow and a light rim,
//! under a window whose content egui draws on a transparent background.
//!
//! What was measured (Windows 11 build 26300, `examples/glass_probe.rs`, screen captures at 150%
//! and 300%):
//!
//! - DWM's system backdrop (`DWMWA_SYSTEMBACKDROP_TYPE`, what `window::frost_window` uses) paints
//!   the whole window rectangle; a rounded window region does not clip it. On a window that is
//!   never active it is a flat grey, not glass.
//! - The undocumented accent policy (`SetWindowCompositionAttribute`, acrylic blur-behind) does
//!   blur, but also over the whole rectangle: the rounded region is ignored there too.
//! - Windows.UI.Composition works: the host backdrop brush (the blurred desktop behind the window)
//!   clipped by a rounded-rectangle geometry gives real blur with smooth, anti-aliased corners at
//!   any radius, and a drop shadow masked by the same shape. Its visuals cannot go under an egui
//!   window's own content, and its shadow needs room outside the shape, so they live in a second
//!   window just below.
//!
//! So a panel is two windows: the served window keeps its size, loses Windows' corners, border
//! and backdrop, and gets a rounded window region (so clicks outside the shape fall through); a
//! backdrop window of its own class sits right under it, shape plus shadow, layered and
//! transparent to the mouse, and shows only the composition tree. A subclass on the served window
//! moves, sizes, shows and hides the backdrop in the same message that moved the window; screen
//! captures taken while a window moved every 10 ms found the two never apart. During a live resize
//! about one capture in forty showed them one step apart.
//!
//! When transparency effects are off, the tree fills the shape with a solid colour instead of the
//! blur (rounded corners and shadow stay). When composition is not available at all (or
//! `SHELLMAN_NO_COMPOSITION` is set, to see this path), the window paints its own rounded surface
//! with [`content_frame`], without blur or shadow.
//!
//! DWM serves the host backdrop as a flat colour while a session WNF state says transparency is
//! not allowed (or nobody has published it: without Explorer or Windows' app host, until the
//! session publishes it; see `shellman_core::system::transparency`), and a backdrop set up then
//! stays flat after DWM starts allowing it (measured: the bar's glass stayed flat once the state
//! was published, while new windows blurred). So the backdrop window and its visuals are made
//! again when DWM may have changed its mind: DWM restarted or changed colours, the immersive
//! colours changed, or another shellman process asks with [`REFRESH_MESSAGE`] (the session does
//! after it changed the state, the desktop once the app host is up). Rebuilds are coalesced, at
//! most one per [`REBUILD_GAP`]. `SHELLMAN_GLASS_LOG=1` prints what made a panel do so. An
//! attempt that made nothing is tried again (`retry`): while only the thread's rate bucket stood
//! in the way, once it has a token, for up to 10 s (also a panel whose first attach was refused
//! so); when Windows refused, a few times. A new ask starts the count over.
//!
//! The panel defends DWM against its callers: every size goes through `guard` (a piece of no
//! size is hidden, never sized to 0; nothing past 8192 px; no NaN), and a thread makes at most a
//! few backdrop windows a second. Still, callers should not make glass per click or per frame,
//! nor a monitor-sized glass popup: make one window, sized to its content, once, and keep it.
//!
//! Using it: attach a panel once the window exists, paint the content inside
//! [`content_frame`]`(panel.composed(), ..)`, and pass the theme's style to
//! [`GlassPanel::set_style`] when it may have changed. Everything else follows on its own,
//! except cloaking, which sends no message: cloak the window with [`cloak`], which takes its
//! glass along (from any thread).
//!
//! The surface fills the window unless [`GlassPanel::set_shape`] puts it on a part of it. A
//! window whose surface changes size often (the dock, as apps come and go) can stay one size and
//! change the shape instead: a resized window shows its old content for a frame, and a resized
//! backdrop window shows its old visuals, while a shape changed inside both windows reaches the
//! screen with the window's next frame.
//!
//! Or the window can have several separate pieces ("islands", see [`islands`]) through
//! [`GlassPanel::set_islands`]: then the whole window stays clickable (the gaps between the
//! islands are the window's own, to drag it by) and only the islands are glass;
//! [`GlassPanel::set_islands_only`] instead lets clicks between them through to what is below.

mod backdrop;
mod cloak;
mod effect;
pub mod guard;
pub mod islands;
mod retry;
mod style;
mod tree;

pub use cloak::{cloak, is_cloaked};
pub use islands::Island;
pub use style::{content_frame, Geometry, Luminosity, Shadow, Style};

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tree::{Paint, Placed, Tree};
use windows::core::w;
use windows::Foundation::TypedEventHandler;
use windows::Win32::Foundation::POINT;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMSBT_NONE, DWMWA_BORDER_COLOR, DWMWA_SYSTEMBACKDROP_TYPE,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DWMWINDOWATTRIBUTE,
};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::Graphics::Gdi::{
    CombineRgn, CreateRectRgn, CreateRoundRectRgn, DeleteObject, EqualRgn, GetWindowRgn,
    SetWindowRgn, RGN_ERROR, RGN_OR,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, GetWindowRect, IsIconic, IsWindowVisible, KillTimer, PostMessageW,
    RegisterWindowMessageW, SetTimer, SetWindowPos, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, WM_DPICHANGED, WM_DWMCOLORIZATIONCOLORCHANGED,
    WM_DWMCOMPOSITIONCHANGED, WM_NCDESTROY, WM_SETTINGCHANGE, WM_TIMER, WM_WINDOWPOSCHANGED,
};
use windows::UI::ViewManagement::UISettings;

/// The subclass id on the served window.
const SUBCLASS: usize = 0x6c61_7373;
/// The timer on the served window that runs a rebuild put off by [`REBUILD_GAP`].
const REBUILD_TIMER: usize = 0x6c61_7374;
/// The least time between two rebuilds of one panel.
pub const REBUILD_GAP: Duration = Duration::from_millis(500);

thread_local! {
    /// How many backdrop windows this thread may still make right now (`guard::Bucket`).
    static BACKDROPS: Cell<guard::Bucket> = Cell::new(guard::Bucket::default());
}

/// A rounded glass surface under one window. Dropping it removes the surface.
pub struct GlassPanel {
    inner: Rc<Inner>,
}

struct Inner {
    /// The window the panel serves, whose content sits on top.
    target: HWND,
    /// The backdrop window and its visuals; None when Windows cannot compose them.
    composed: RefCell<Option<Composed>>,
    /// A rebuild of `composed` has been asked for and not yet done.
    rebuild_pending: Cell<bool>,
    /// A rebuild is running (never two at once).
    rebuilding: Cell<bool>,
    /// When the last rebuild ran, for [`REBUILD_GAP`].
    rebuilt: Cell<Option<Instant>>,
    /// When to try again after an attempt that made no backdrop (`retry`).
    retry: Cell<retry::Retry>,
    /// The first attach was refused only because backdrops were being made too often: the
    /// panel keeps trying to get one, like a rebuild.
    wanted: Cell<bool>,
    style: Cell<Style>,
    /// Where the surface is, in the window's pixels from its top-left; None for all of it.
    shape: Cell<Option<RECT>>,
    /// A part of the window outside the surface that still shows and takes clicks.
    also: Cell<Option<RECT>>,
    /// Several pieces instead of one shape, and the pixels per point they were laid out at.
    islands: RefCell<Option<(Vec<Island>, f32)>>,
    /// Counts changes to `islands`, so a layout can tell it is out of date.
    islands_seen: Cell<u64>,
    /// Which part of an islands window takes clicks.
    islands_hit: Cell<Hit>,
    /// What the shape was last laid out for: the window's size, the shape, the extra part and
    /// the DPI. None forces a layout.
    laid_out: Cell<Option<Layout>>,
    /// Whether Windows' transparency effects are on.
    effects: Cell<bool>,
    /// False once the served window is destroyed (and the backdrop with it).
    alive: Cell<bool>,
}

/// What an islands window shows and takes clicks on.
#[derive(Clone, Copy, PartialEq)]
enum Hit {
    /// All of it.
    Window,
    /// The islands, and the given part of the window, in its pixels.
    Islands(Option<RECT>),
}

/// The islands' edges (window pixels) and radii, and the extra part, that make up the region of
/// a window whose islands alone take clicks.
type Cut = (Vec<([i32; 4], f32)>, Option<RECT>);

#[derive(Clone, Copy, PartialEq)]
struct Layout {
    window: (i32, i32),
    shape: RECT,
    also: Option<RECT>,
    dpi: u32,
    /// The islands' change count, and where the client area starts in the window.
    islands: Option<(u64, (i32, i32))>,
    /// Whether the region was cut to the islands (`Hit::Islands`).
    cut: bool,
}

struct Composed {
    window: HWND,
    tree: Tree,
    /// Tells the panel when transparency effects are turned on or off.
    settings: Option<(UISettings, i64)>,
}

impl GlassPanel {
    /// Put a rounded surface under `hwnd`, a top-level window of this thread. The window loses
    /// Windows' own corners, border and backdrop, and gets a rounded window region so clicks
    /// outside the shape fall through. See [`GlassPanel::composed`] for whether there is glass.
    pub fn attach(hwnd: isize, style: Style) -> Self {
        let target = HWND(hwnd as *mut _);
        bare(target);
        let made = compose(hwnd);
        // Refused for the rate alone: tried again once the bucket has a token.
        let refused_for_rate = match &made {
            Err(Refused::RateLimited(ready_in)) => Some(*ready_in),
            _ => None,
        };
        let inner = Rc::new(Inner {
            target,
            composed: RefCell::new(made.ok()),
            rebuild_pending: Cell::new(false),
            rebuilding: Cell::new(false),
            rebuilt: Cell::new(None),
            retry: Cell::new(retry::Retry::default()),
            wanted: Cell::new(refused_for_rate.is_some()),
            style: Cell::new(style),
            shape: Cell::new(None),
            also: Cell::new(None),
            islands: RefCell::new(None),
            islands_seen: Cell::new(0),
            islands_hit: Cell::new(Hit::Window),
            laid_out: Cell::new(None),
            effects: Cell::new(effects_enabled()),
            alive: Cell::new(true),
        });
        if let Some(composed) = &*inner.composed.borrow() {
            cloak::remember(target, composed.window);
        }
        unsafe {
            let _ = SetWindowSubclass(
                target,
                Some(follow_proc),
                SUBCLASS,
                Rc::as_ptr(&inner) as usize,
            );
        }
        inner.follow();
        if let Some(ready_in) = refused_for_rate {
            inner.retry_after(retry::Outcome::RateLimited(ready_in));
        }
        Self { inner }
    }

    /// Whether the surface is drawn under the window (blur, tint, rim and shadow). If not, the
    /// window paints it itself: see [`content_frame`].
    pub fn composed(&self) -> bool {
        self.inner.composed.borrow().is_some()
    }

    /// Change the look (the theme, accent or glass amount changed). Free when nothing changed.
    pub fn set_style(&self, style: Style) {
        if self.inner.style.replace(style) != style {
            self.inner.laid_out.set(None);
            self.inner.follow();
        }
    }

    /// Put the surface on `shape` (in the window's pixels, from its top-left) instead of the
    /// whole window; None gives it the whole window again. `also` is another part of the window
    /// that shows and takes clicks without glass under it (a label above the surface); the rest
    /// of the window is neither drawn nor clicked. Free when nothing changed.
    pub fn set_shape(&self, shape: Option<RECT>, also: Option<RECT>) {
        let mut changed = self.inner.shape.replace(shape) != shape;
        changed |= self.inner.islands.borrow_mut().take().is_some();
        if (self.inner.also.replace(also) != also) | changed {
            self.inner.follow();
        }
    }

    /// Put the glass on several separate pieces instead of one shape, in points of the window's
    /// client area at `scale` pixels per point (egui's `pixels_per_point`). The whole window
    /// keeps taking clicks; only the islands are glass. An empty list shows no glass. Free when
    /// nothing changed, so it can be called every frame; [`GlassPanel::set_shape`] goes back to
    /// one shape.
    pub fn set_islands(&self, islands: &[Island], scale: f32) {
        self.put_islands(islands, scale, Hit::Window);
    }

    /// Like [`GlassPanel::set_islands`], but only the islands, and `also` (another part of the
    /// window, in its pixels from its top-left, such as a label beside them), show and take
    /// clicks; elsewhere clicks go through to whatever is under the window. For a window much
    /// bigger than its islands, such as a dock window as wide as its monitor.
    pub fn set_islands_only(&self, islands: &[Island], scale: f32, also: Option<RECT>) {
        self.put_islands(islands, scale, Hit::Islands(also));
    }

    fn put_islands(&self, islands: &[Island], scale: f32, hit: Hit) {
        // A NaN scale would never compare equal, and lay the islands out again every frame.
        let scale = guard::scale(scale);
        {
            let mut current = self.inner.islands.borrow_mut();
            if current
                .as_ref()
                .is_some_and(|(list, s)| list == islands && *s == scale)
                && self.inner.islands_hit.get() == hit
            {
                return;
            }
            *current = Some((islands.to_vec(), scale));
        }
        self.inner.islands_hit.set(hit);
        self.inner
            .islands_seen
            .set(self.inner.islands_seen.get() + 1);
        self.inner.follow();
    }

    /// The look the panel has now.
    pub fn style(&self) -> Style {
        self.inner.style.get()
    }
}

impl Drop for GlassPanel {
    fn drop(&mut self) {
        let inner = &self.inner;
        let composed = inner.composed.borrow_mut().take();
        if let Some(composed) = &composed {
            unwatch(composed);
        }
        if !inner.alive.get() {
            return;
        }
        cloak::forget(inner.target);
        unsafe {
            let _ = KillTimer(Some(inner.target), REBUILD_TIMER);
            let _ = RemoveWindowSubclass(inner.target, Some(follow_proc), SUBCLASS);
            if let Some(composed) = &composed {
                let _ = DestroyWindow(composed.window);
            }
            let _ = SetWindowRgn(inner.target, None, true);
        }
    }
}

/// Why no backdrop was made.
enum Refused {
    /// Backdrops were made too often on this thread (`guard::Bucket`): one can be in this long.
    RateLimited(Duration),
    /// Composition is off or not there, or Windows would not make it.
    Unavailable,
}

/// The backdrop window and its visuals, if Windows can compose them.
fn compose(hwnd: isize) -> Result<Composed, Refused> {
    if std::env::var_os("SHELLMAN_NO_COMPOSITION").is_some() {
        return Err(Refused::Unavailable);
    }
    let compositor = tree::compositor().ok_or(Refused::Unavailable)?;
    // A caller making glass per click or per frame would have DWM create and destroy a target
    // and its textures at that pace; past a short burst, a few a second.
    let taken = BACKDROPS.with(|b| {
        let mut bucket = b.get();
        let now = Instant::now();
        let ok = bucket.take(now);
        b.set(bucket);
        if ok {
            Ok(())
        } else {
            Err(bucket.ready_in(now))
        }
    });
    if let Err(ready_in) = taken {
        trace(|| format!("glass {hwnd:#x}: backdrop windows made too often, none made now"));
        return Err(Refused::RateLimited(ready_in));
    }
    let window = backdrop::create().ok_or(Refused::Unavailable)?;
    match Tree::new(&compositor, window) {
        Ok(tree) => Ok(Composed {
            window,
            tree,
            settings: watch_effects(hwnd),
        }),
        Err(_) => {
            unsafe {
                let _ = DestroyWindow(window);
            }
            Err(Refused::Unavailable)
        }
    }
}

/// Stop hearing about transparency effects for `composed`.
fn unwatch(composed: &Composed) {
    if let Some((settings, token)) = &composed.settings {
        let _ = settings.RemoveAdvancedEffectsEnabledChanged(*token);
    }
}

impl Inner {
    /// Make the backdrop window and its visuals again, in place of the old ones, and lay them
    /// out. Keeps the old ones when new ones cannot be made.
    fn rebuild(&self) {
        if self.rebuilding.get() {
            return;
        }
        if !self.alive.get() || (self.composed.borrow().is_none() && !self.wanted.get()) {
            self.rebuild_pending.set(false);
            return;
        }
        // At most one per REBUILD_GAP: a later one waits for the timer, and asks meanwhile
        // join it (`rebuild_pending` stays set).
        let wait = self.rebuilt.get().map_or(Duration::ZERO, |at| {
            REBUILD_GAP.saturating_sub(at.elapsed())
        });
        if !wait.is_zero() {
            self.later(wait);
            return;
        }
        self.rebuild_pending.set(false);
        self.rebuilt.set(Some(Instant::now()));
        self.rebuilding.set(true);
        let fresh = match compose(self.target.0 as isize) {
            Ok(fresh) => fresh,
            Err(refused) => {
                self.rebuilding.set(false);
                // Keep the old one, and try again: when the bucket has a token, or (Windows
                // refused) a few times.
                self.retry_after(match refused {
                    Refused::RateLimited(ready_in) => retry::Outcome::RateLimited(ready_in),
                    Refused::Unavailable => retry::Outcome::Failed,
                });
                return;
            }
        };
        self.retry_after(retry::Outcome::Made);
        // A panel that had no glass yet painted its own surface: have it paint again, now on
        // glass.
        if self.wanted.replace(false) {
            unsafe {
                let _ =
                    windows::Win32::Graphics::Gdi::InvalidateRect(Some(self.target), None, false);
            }
        }
        cloak::remember(self.target, fresh.window);
        let old = self.composed.replace(Some(fresh));
        self.effects.set(effects_enabled());
        self.laid_out.set(None);
        // The new backdrop goes under the window first, then the old one goes.
        self.follow();
        if let Some(old) = old {
            unwatch(&old);
            unsafe {
                let _ = DestroyWindow(old.window);
            }
        }
        self.rebuilding.set(false);
        trace(|| format!("glass {:?}: backdrop made again", self.target.0));
    }

    /// After an attempt came to `outcome`: try again when [`retry::Retry`] says.
    fn retry_after(&self, outcome: retry::Outcome) {
        let mut state = self.retry.get();
        let next = state.after(outcome, Instant::now(), REBUILD_GAP);
        self.retry.set(state);
        match next {
            retry::Next::RetryIn(wait) => {
                self.rebuild_pending.set(true);
                self.later(wait);
            }
            retry::Next::Done => {}
            retry::Next::GiveUp => trace(|| {
                format!(
                    "glass {:?}: no backdrop made, given up until asked again",
                    self.target.0
                )
            }),
        }
    }

    /// Run the pending rebuild after `wait` (the served window's timer; one at a time).
    fn later(&self, wait: Duration) {
        let ms = wait.as_millis().clamp(10, u32::MAX as u128) as u32;
        unsafe {
            SetTimer(Some(self.target), REBUILD_TIMER, ms, None);
        }
    }

    /// Ask for a rebuild once the current message is done; several asks make one rebuild.
    fn ask_rebuild(&self) {
        // A new ask: every attempt again, whatever earlier ones came to.
        let mut state = self.retry.get();
        state.asked();
        self.retry.set(state);
        if !self.rebuild_pending.replace(true) {
            unsafe {
                let _ = PostMessageW(Some(self.target), rebuild_message(), WPARAM(0), LPARAM(0));
            }
        }
    }

    /// Match the window: its rounded region, and the backdrop in the same place, at the same
    /// visibility, right under it.
    fn follow(&self) {
        unsafe {
            let visible =
                IsWindowVisible(self.target).as_bool() && !IsIconic(self.target).as_bool();
            if !visible {
                if let Some(composed) = &*self.composed.borrow() {
                    let flags = SWP_HIDEWINDOW | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER;
                    let _ = SetWindowPos(composed.window, None, 0, 0, 0, 0, flags | SWP_NOACTIVATE);
                }
                return;
            }
            let mut rc = RECT::default();
            if GetWindowRect(self.target, &mut rc).is_err() {
                return;
            }
            let window = (rc.right - rc.left, rc.bottom - rc.top);
            let dpi = GetDpiForWindow(self.target).max(96);
            let scale = dpi as f32 / 96.0;
            let style = self.style.get();
            let islands = self.islands.borrow();
            // With `Hit::Islands`, the islands in the window's pixels and their radii: the
            // window's region, which is all of it that shows and takes clicks.
            let mut cut: Option<Cut> = None;
            let (geo, area, placed, also, key) = match islands.as_ref() {
                Some((list, points)) => {
                    // The backdrop covers the whole window and the shadow's room around it; the
                    // islands are placed inside it, from where the client area starts.
                    let geo = Geometry::new(window, scale, &style);
                    let client = client_origin(self.target, &rc);
                    if let Hit::Islands(extra) = self.islands_hit.get() {
                        let pieces = list
                            .iter()
                            .map(|i| {
                                let (e, radius) = islands::to_pixels(i, *points);
                                let e = [
                                    e[0] + client.0,
                                    e[1] + client.1,
                                    e[2] + client.0,
                                    e[3] + client.1,
                                ];
                                (e, radius)
                            })
                            .collect();
                        cut = Some((pieces, extra));
                    }
                    let placed = list
                        .iter()
                        .map(|i| {
                            let (e, radius) = islands::to_pixels(i, *points);
                            Placed {
                                left: geo.left + client.0 + e[0],
                                top: geo.top + client.1 + e[1],
                                size: (e[2] - e[0], e[3] - e[1]),
                                radius,
                                opacity: i.opacity,
                                shadow: i.shadow,
                            }
                        })
                        .filter(|p| p.size.0 > 0 && p.size.1 > 0)
                        .collect();
                    let whole = RECT {
                        left: 0,
                        top: 0,
                        right: window.0,
                        bottom: window.1,
                    };
                    (
                        geo,
                        whole,
                        placed,
                        None,
                        Some((self.islands_seen.get(), client)),
                    )
                }
                None => {
                    let area = self.shape.get().unwrap_or(RECT {
                        left: 0,
                        top: 0,
                        right: window.0,
                        bottom: window.1,
                    });
                    let shape = (
                        area.right.saturating_sub(area.left),
                        area.bottom.saturating_sub(area.top),
                    );
                    let mut geo = Geometry::new(shape, scale, &style);
                    // The backdrop window covers the whole served window (and the shadow's room
                    // around it), and the shape moves inside it. So a shape that changes every
                    // frame (the dock growing as an app opens) only changes the visuals, which
                    // land on screen together with the window's own next frame; moving the
                    // backdrop window as well would show it a frame ahead of them.
                    // Saturating: a caller's shape may be anything (`guard` sorts out the rest).
                    geo.left = geo.left.saturating_add(area.left);
                    geo.top = geo.top.saturating_add(area.top);
                    geo.right = geo
                        .right
                        .saturating_add(window.0.saturating_sub(area.right));
                    geo.bottom = geo
                        .bottom
                        .saturating_add(window.1.saturating_sub(area.bottom));
                    let placed = vec![Placed {
                        left: geo.left,
                        top: geo.top,
                        size: geo.shape,
                        radius: geo.radius,
                        opacity: 1.0,
                        shadow: true,
                    }];
                    (geo, area, placed, self.also.get(), None)
                }
            };
            drop(islands);
            let layout = Layout {
                window,
                shape: area,
                also,
                dpi,
                islands: key,
                cut: cut.is_some(),
            };
            let before = self.laid_out.replace(Some(layout));
            if before != Some(layout) {
                let moved = before.is_none_or(|b| {
                    (b.window, b.shape, b.dpi, b.islands) != (window, area, dpi, key)
                });
                if let Some(composed) = self.composed.borrow().as_ref().filter(|_| moved) {
                    let paint = Paint {
                        style,
                        effects: self.effects.get(),
                        shadow_blur: geo.shadow_blur,
                        shadow_offset: geo.shadow_offset,
                    };
                    let _ = composed.tree.apply(&placed, paint);
                }
                // Islands keep the whole window (the gaps move it), so its region changes only
                // with its size; one shape is all of the window that shows and takes clicks.
                // Either way the region also takes away Windows' own shadow and border.
                // Setting it sends the window another move message, which finds the layout done.
                if let Some((pieces, extra)) = &cut {
                    // Only the islands (and the extra part) show and take clicks.
                    let region = CreateRectRgn(0, 0, 0, 0);
                    for (e, radius) in pieces {
                        let r = ((radius - 2.0).max(0.0).round() as i32) * 2;
                        let piece = CreateRoundRectRgn(
                            e[0],
                            e[1],
                            e[2].saturating_add(1),
                            e[3].saturating_add(1),
                            r,
                            r,
                        );
                        CombineRgn(Some(region), Some(region), Some(piece), RGN_OR);
                        let _ = DeleteObject(piece.into());
                    }
                    if let Some(x) = extra {
                        let piece = CreateRectRgn(x.left, x.top, x.right, x.bottom);
                        CombineRgn(Some(region), Some(region), Some(piece), RGN_OR);
                        let _ = DeleteObject(piece.into());
                    }
                    // Islands that moved inside the extra part (the shelf's fan, opening) leave
                    // the region as it was: setting it again makes Windows clip and redraw the
                    // window in the middle of an animation, once per frame.
                    let current = CreateRectRgn(0, 0, 0, 0);
                    let same = GetWindowRgn(self.target, current) != RGN_ERROR
                        && EqualRgn(current, region).as_bool();
                    let _ = DeleteObject(current.into());
                    if same {
                        let _ = DeleteObject(region.into());
                    } else {
                        SetWindowRgn(self.target, Some(region), true);
                    }
                } else if key.is_some() {
                    // A window that only now takes all of itself again needs its region reset.
                    let same =
                        before.is_some_and(|b| b.islands.is_some() && b.window == window && !b.cut);
                    if !same {
                        let region = CreateRectRgn(0, 0, window.0, window.1);
                        SetWindowRgn(self.target, Some(region), true);
                    }
                } else {
                    let r = geo.region_radius() * 2;
                    let region = CreateRoundRectRgn(
                        area.left,
                        area.top,
                        area.right.saturating_add(1),
                        area.bottom.saturating_add(1),
                        r,
                        r,
                    );
                    if let Some(also) = layout.also {
                        let extra = CreateRectRgn(also.left, also.top, also.right, also.bottom);
                        CombineRgn(Some(region), Some(region), Some(extra), RGN_OR);
                        let _ = DeleteObject(extra.into());
                    }
                    SetWindowRgn(self.target, Some(region), true);
                }
            }
            if let Some(composed) = &*self.composed.borrow() {
                // A window of no size (or one smaller than the shape's room) shows no backdrop
                // window at all rather than one sized to 0.
                match guard::window(geo.window()).filter(|_| window.0 > 0 && window.1 > 0) {
                    Some((w, h)) => {
                        let _ = SetWindowPos(
                            composed.window,
                            Some(self.target),
                            rc.left.saturating_sub(geo.left.saturating_sub(area.left)),
                            rc.top.saturating_sub(geo.top.saturating_sub(area.top)),
                            w,
                            h,
                            SWP_SHOWWINDOW | SWP_NOACTIVATE,
                        );
                    }
                    None => {
                        let flags = SWP_HIDEWINDOW | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER;
                        let _ =
                            SetWindowPos(composed.window, None, 0, 0, 0, 0, flags | SWP_NOACTIVATE);
                    }
                }
            }
        }
    }
}

/// Where the client area starts inside the window `rc` (a maximized window's frame hangs off the
/// screen around it).
fn client_origin(hwnd: HWND, rc: &RECT) -> (i32, i32) {
    let mut p = POINT { x: 0, y: 0 };
    unsafe {
        let _ = ClientToScreen(hwnd, &mut p);
    }
    (p.x - rc.left, p.y - rc.top)
}

/// Take away what Windows draws around and behind the window: its corners, its border and any
/// system backdrop. The panel draws all of that.
fn bare(hwnd: HWND) {
    let set = |attribute: DWMWINDOWATTRIBUTE, value: u32| unsafe {
        let _ = DwmSetWindowAttribute(hwnd, attribute, (&raw const value).cast(), 4);
    };
    set(DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND.0 as u32);
    // DWMWA_COLOR_NONE.
    set(DWMWA_BORDER_COLOR, 0xFFFF_FFFE);
    set(DWMWA_SYSTEMBACKDROP_TYPE, DWMSBT_NONE.0 as u32);
}

/// Whether Windows' transparency effects are on (Settings, Personalization, Colors). Assumed on
/// when Windows cannot say.
pub fn effects_enabled() -> bool {
    UISettings::new()
        .and_then(|s| s.AdvancedEffectsEnabled())
        .unwrap_or(true)
}

/// The message the served window gets when transparency effects are turned on or off.
fn effects_message() -> u32 {
    static MESSAGE: OnceLock<u32> = OnceLock::new();
    *MESSAGE.get_or_init(|| unsafe { RegisterWindowMessageW(w!("blurman-glass-effects")) })
}

/// Have Windows say when transparency effects change. Its callback comes on another thread, so
/// it only posts a message to the window.
fn watch_effects(hwnd: isize) -> Option<(UISettings, i64)> {
    let settings = UISettings::new().ok()?;
    let message = effects_message();
    let token = settings
        .AdvancedEffectsEnabledChanged(&TypedEventHandler::new(move |_, _| {
            unsafe {
                let _ = PostMessageW(Some(HWND(hwnd as *mut _)), message, WPARAM(0), LPARAM(0));
            }
            Ok(())
        }))
        .ok()?;
    Some((settings, token))
}

/// The name of the message that makes every glass panel make its backdrop again, broadcast by
/// another shellman process when DWM may have started serving backdrops (the session published
/// the transparency state, or Windows' app host brought the immersive shell up). Register it
/// with `RegisterWindowMessageW`.
pub const REFRESH_MESSAGE: &str = "shellman-glass-refresh";

fn refresh_message() -> u32 {
    static MESSAGE: OnceLock<u32> = OnceLock::new();
    *MESSAGE.get_or_init(|| unsafe { RegisterWindowMessageW(w!("blurman-glass-refresh")) })
}

/// The panel's own message for a rebuild asked for earlier.
fn rebuild_message() -> u32 {
    static MESSAGE: OnceLock<u32> = OnceLock::new();
    *MESSAGE.get_or_init(|| unsafe { RegisterWindowMessageW(w!("blurman-glass-rebuild")) })
}

/// Whether a WM_SETTINGCHANGE is about the immersive colours ("ImmersiveColorSet").
fn is_immersive_colors(lparam: LPARAM) -> bool {
    if lparam.0 == 0 {
        return false;
    }
    let text = unsafe { windows::core::PCWSTR(lparam.0 as *const u16).to_string() };
    text.is_ok_and(|t| t == "ImmersiveColorSet")
}

/// With `SHELLMAN_GLASS_LOG` set, a line on stderr about the glass; nothing otherwise.
fn trace(line: impl FnOnce() -> String) {
    static ON: OnceLock<bool> = OnceLock::new();
    if *ON.get_or_init(|| std::env::var_os("SHELLMAN_GLASS_LOG").is_some()) {
        eprintln!("{}", line());
    }
}

unsafe extern "system" fn follow_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    let inner = &*(data as *const Inner);
    match msg {
        WM_WINDOWPOSCHANGED => {
            inner.follow();
            DefSubclassProc(hwnd, msg, wparam, lparam)
        }
        WM_DPICHANGED => {
            let result = DefSubclassProc(hwnd, msg, wparam, lparam);
            inner.follow();
            result
        }
        WM_NCDESTROY => {
            inner.alive.set(false);
            cloak::forget(hwnd);
            let _ = RemoveWindowSubclass(hwnd, Some(follow_proc), SUBCLASS);
            if let Some(composed) = &*inner.composed.borrow() {
                let _ = DestroyWindow(composed.window);
            }
            DefSubclassProc(hwnd, msg, wparam, lparam)
        }
        // DWM restarted or changed its colours (Windows tells every top-level window), or the
        // immersive colours changed: DWM may serve the host backdrop differently now, and a
        // backdrop it served flat stays flat, so make it again.
        WM_DWMCOMPOSITIONCHANGED | WM_DWMCOLORIZATIONCOLORCHANGED => {
            trace(|| format!("glass {:?}: message {msg:#x}", hwnd.0));
            inner.ask_rebuild();
            DefSubclassProc(hwnd, msg, wparam, lparam)
        }
        WM_SETTINGCHANGE if is_immersive_colors(lparam) => {
            trace(|| format!("glass {:?}: ImmersiveColorSet", hwnd.0));
            inner.ask_rebuild();
            DefSubclassProc(hwnd, msg, wparam, lparam)
        }
        _ if msg == refresh_message() => {
            trace(|| format!("glass {:?}: refresh asked", hwnd.0));
            inner.ask_rebuild();
            LRESULT(0)
        }
        _ if msg == rebuild_message() => {
            inner.rebuild();
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == REBUILD_TIMER => {
            let _ = KillTimer(Some(hwnd), REBUILD_TIMER);
            if inner.rebuild_pending.get() {
                inner.rebuild();
            }
            LRESULT(0)
        }
        _ if msg == effects_message() => {
            inner.effects.set(effects_enabled());
            inner.laid_out.set(None);
            inner.follow();
            LRESULT(0)
        }
        _ => DefSubclassProc(hwnd, msg, wparam, lparam),
    }
}
