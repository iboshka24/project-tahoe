// SPDX-License-Identifier: MIT OR Apache-2.0
//! Compositor state and protocol handlers.
//!
//! Window *policy* lives in [`crate::wm`]; this module only translates
//! Wayland/Smithay events into calls on it and back. Keeping the split means
//! the interesting behaviour (z-order, focus, dragging, placement) is
//! unit-tested without a socket, and this file stays a thin adapter.

use std::{
    collections::HashMap,
    os::unix::io::OwnedFd,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

use smithay::{
    backend::{
        input::{ButtonState, KeyState, Keycode},
        renderer::utils::{RendererSurfaceStateUserData, on_commit_buffer_handler},
    },
    input::{
        Seat, SeatHandler, SeatState,
        keyboard::KeyboardHandle,
        pointer::{ButtonEvent, MotionEvent, PointerHandle},
    },
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::{
            Client, DisplayHandle,
            protocol::{wl_buffer, wl_output::WlOutput, wl_seat, wl_surface::WlSurface},
        },
    },
    utils::{Logical, Point, Rectangle, SERIAL_COUNTER, Serial, Size},
    wayland::{
        buffer::BufferHandler,
        compositor::{
            CompositorClientState, CompositorHandler, CompositorState, SurfaceAttributes,
            TraversalAction, with_states, with_surface_tree_downward,
        },
        output::OutputHandler,
        selection::{
            SelectionHandler,
            data_device::{
                ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
            },
        },
        shell::{
            wlr_layer::{
                Layer, LayerSurface, LayerSurfaceCachedState, WlrLayerShellHandler,
                WlrLayerShellState,
            },
            xdg::{
                PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
                XdgToplevelSurfaceData,
            },
        },
        shm::{ShmHandler, ShmState},
    },
};
use tahoe_shell::AppId;

use smithay::{
    delegate_compositor, delegate_data_device, delegate_layer_shell, delegate_output,
    delegate_seat, delegate_shm, delegate_xdg_shell,
};

use crate::wm::{Cascade, Rect, WindowId, WindowStack};

/// Height of the reserved top panel area, in logical pixels.
///
/// The shell panel occupies this strip; windows are placed below it so they
/// are never covered. Exposed as a constant so the shell and the compositor
/// cannot disagree about it.
pub const PANEL_HEIGHT: i32 = 28;

/// Everything a Wayland client can trigger, counted for diagnostics.
///
/// The counters exist so tests and `tahoe-diagnostics` can assert that the
/// compositor really did something, rather than checking that it is merely
/// alive.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CompositorStats {
    /// Surfaces committed with a buffer for the first time.
    pub first_commits: u64,
    /// Total surface commits observed.
    pub commits: u64,
    /// Toplevels created by clients.
    pub toplevels_created: u64,
    /// Toplevels destroyed.
    pub toplevels_destroyed: u64,
    /// Layer surfaces created (panel, dock, notifications).
    pub layer_surfaces_created: u64,
    /// Layer surfaces destroyed.
    pub layer_surfaces_destroyed: u64,
    /// Frame callbacks sent.
    pub frames_sent: u64,
    /// Pointer buttons delivered.
    pub buttons: u64,
    /// Key events delivered.
    pub keys: u64,
    /// Interactive moves started.
    pub moves: u64,
}

/// Client connect/disconnect counters.
///
/// These live behind an [`Arc`] with atomics rather than in
/// [`CompositorStats`] because the disconnect is reported by Smithay's
/// `ClientData` callback, which only gets `&self` and no access to the
/// compositor state.
#[derive(Debug, Default)]
pub struct ClientCounters {
    connected: AtomicU64,
    disconnected: AtomicU64,
}

impl ClientCounters {
    /// Clients that completed the display handshake.
    #[must_use]
    pub fn connected(&self) -> u64 {
        self.connected.load(Ordering::Relaxed)
    }

    /// Clients whose connection has closed.
    #[must_use]
    pub fn disconnected(&self) -> u64 {
        self.disconnected.load(Ordering::Relaxed)
    }

    /// Count a connection.
    pub fn count_connected(&self) {
        self.connected.fetch_add(1, Ordering::Relaxed);
    }

    /// Count a disconnect.
    pub fn count_disconnected(&self) {
        self.disconnected.fetch_add(1, Ordering::Relaxed);
    }

    /// True once at least one client has both connected and disconnected,
    /// which is the minimum evidence of a working client lifecycle.
    #[must_use]
    pub fn saw_full_lifecycle(&self) -> bool {
        self.connected() > 0 && self.disconnected() > 0
    }
}

/// A layer surface plus the namespace its client asked for.
#[derive(Debug, Clone)]
pub struct ManagedLayerSurface {
    /// The shell surface.
    pub surface: LayerSurface,
    /// Client-declared namespace (`panel`, `dock`, `notification`, …).
    pub namespace: String,
    /// Layer the client asked to be placed in.
    pub layer: Layer,
    /// Size the compositor suggested.
    pub size: Size<i32, Logical>,
}

/// State shared by every backend.
pub struct Tahoe {
    /// When the compositor started (monotonic clock for frame timestamps).
    pub start: Instant,
    /// Size of the output in logical pixels.
    pub output_size: Size<i32, Logical>,
    /// The `wl_output` global.
    pub output: Output,
    /// Window policy and bookkeeping.
    pub windows: WindowStack,
    /// Cascade policy for new windows.
    pub cascade: Cascade,

    /// Wayland protocol state.
    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub layer_shell_state: WlrLayerShellState,
    pub shm_state: ShmState,
    pub seat_state: SeatState<Self>,
    pub data_device_state: DataDeviceState,

    /// Input handles. Cloned rather than borrowed so handlers can call
    /// `set_focus(self, ...)` while `&mut self` is borrowed.
    pub seat: Seat<Self>,
    pub keyboard: KeyboardHandle<Self>,
    pub pointer: PointerHandle<Self>,

    /// Toplevel surfaces by window id.
    toplevels: HashMap<WindowId, ToplevelSurface>,
    /// Layer surfaces in creation order.
    pub layers: Vec<ManagedLayerSurface>,

    /// Last known pointer position in compositor coordinates.
    pub pointer_location: Point<f64, Logical>,
    /// Serial of the last event we can use for focus requests.
    pub last_serial: Serial,

    /// Client lifecycle counters, shared with every `ClientState`.
    client_counters: Arc<ClientCounters>,

    /// Counters for diagnostics and tests.
    pub stats: CompositorStats,
}

/// The work area: the output minus the reserved panel strip.
#[must_use]
pub fn work_area(output: Size<i32, Logical>) -> Rect {
    Rect::new(0, PANEL_HEIGHT, output.w, (output.h - PANEL_HEIGHT).max(1))
}

/// A window dimension: 60% of what is available, but never below `min`
/// unless `available` is itself smaller than `min`, in which case the whole
/// available space is used.
///
/// Never panics — `i32::clamp` requires `min <= max` and would abort on a
/// tiny output.
#[must_use]
fn preferred_dimension(available: i32, min: i32) -> i32 {
    let available = available.max(1);
    let ideal = available * 3 / 5;
    ideal.clamp(min.min(available), available)
}

impl Tahoe {
    /// Build the compositor state and register every global it serves.
    ///
    /// Registering globals is the moment clients learn which protocols
    /// exist, so this is the single place that defines the compositor's
    /// protocol surface.
    #[must_use]
    pub fn new(display: &DisplayHandle, output_size: Size<i32, Logical>) -> Self {
        let compositor_state = CompositorState::new::<Self>(display);
        let xdg_shell_state = XdgShellState::new::<Self>(display);
        let layer_shell_state = WlrLayerShellState::new::<Self>(display);
        let shm_state = ShmState::new::<Self>(display, vec![]);
        let data_device_state = DataDeviceState::new::<Self>(display);

        let output = Output::new(
            "tahoe-0".to_owned(),
            PhysicalProperties {
                size: (0, 0).into(),
                subpixel: Subpixel::Unknown,
                make: "Tahoe".to_owned(),
                model: "Virtual Output".to_owned(),
            },
        );
        let mode = Mode {
            size: output_size.to_physical(1),
            refresh: 60_000,
        };
        output.change_current_state(Some(mode), None, None, Some((0, 0).into()));
        output.set_preferred(mode);
        output.create_global::<Self>(display);

        let mut seat_state = SeatState::new();
        let mut seat = seat_state.new_wl_seat(display, "tahoe");
        let keyboard = seat
            .add_keyboard(Default::default(), 200, 200)
            .expect("adding a keyboard to a fresh seat cannot fail");
        let pointer = seat.add_pointer();

        Self {
            start: Instant::now(),
            output_size,
            output,
            windows: WindowStack::new(),
            cascade: Cascade::default(),
            compositor_state,
            xdg_shell_state,
            layer_shell_state,
            shm_state,
            seat_state,
            data_device_state,
            seat,
            keyboard,
            pointer,
            toplevels: HashMap::new(),
            layers: Vec::new(),
            pointer_location: (0.0, 0.0).into(),
            last_serial: Serial::from(0),
            client_counters: Arc::new(ClientCounters::default()),
            stats: CompositorStats::default(),
        }
    }

    /// The shared client counters. Handed to each `ClientState` so the
    /// disconnect callback can record itself.
    #[must_use]
    pub fn client_counters(&self) -> Arc<ClientCounters> {
        Arc::clone(&self.client_counters)
    }

    /// Record that a client connected.
    pub fn count_client_connected(&self) {
        self.client_counters.count_connected();
    }

    /// Clients that completed the handshake.
    #[must_use]
    pub fn clients_connected(&self) -> u64 {
        self.client_counters.connected()
    }

    /// Clients whose connection has closed.
    #[must_use]
    pub fn clients_disconnected(&self) -> u64 {
        self.client_counters.disconnected()
    }

    /// True once a client has both connected and disconnected.
    #[must_use]
    pub fn saw_full_client_lifecycle(&self) -> bool {
        self.client_counters.saw_full_lifecycle()
    }

    /// Milliseconds since startup, saturating at `u32::MAX`.
    #[must_use]
    pub fn now_ms(&self) -> u32 {
        u32::try_from(self.start.elapsed().as_millis()).unwrap_or(u32::MAX)
    }

    /// The [`ToplevelSurface`] for a managed window.
    #[must_use]
    pub fn toplevel_for(&self, id: WindowId) -> Option<ToplevelSurface> {
        self.toplevels.get(&id).cloned()
    }

    /// Find the managed window owning a surface.
    #[must_use]
    pub fn window_id_for(&self, surface: &WlSurface) -> Option<WindowId> {
        self.toplevels
            .iter()
            .find(|(_, t)| t.wl_surface() == surface)
            .map(|(id, _)| *id)
    }

    /// The app id a window's client declared, mapped to the shell's id type.
    #[must_use]
    pub fn window_app_id(&self, id: WindowId) -> Option<AppId> {
        self.windows
            .get(id)
            .map(|w| &w.app_id)
            .filter(|a| !a.is_empty())
            .map(|a| AppId::new(a))
    }

    /// The surface of the currently focused window.
    #[must_use]
    pub fn focused_surface(&self) -> Option<WlSurface> {
        self.windows
            .focused_id()
            .and_then(|id| self.toplevel_for(id))
            .map(|t| t.wl_surface().clone())
    }

    /// The size the compositor suggests to new toplevels: a comfortable
    /// fraction of the work area, so a window never opens off-screen.
    ///
    /// On an output smaller than the comfortable minimum the window simply
    /// fills the work area. `clamp` would panic when `min > max`, which is
    /// exactly what a small output produces, so the bounds are ordered
    /// first.
    #[must_use]
    pub fn suggested_size(&self) -> Size<i32, Logical> {
        let area = work_area(self.output_size);
        Size::from((
            preferred_dimension(area.w, 320),
            preferred_dimension(area.h, 240),
        ))
    }

    /// Focus a surface, or clear focus when `None`.
    pub fn set_keyboard_focus(&mut self, surface: Option<WlSurface>) {
        let serial = SERIAL_COUNTER.next_serial();
        self.last_serial = serial;
        let keyboard = self.keyboard.clone();
        keyboard.set_focus(self, surface, serial);
    }

    /// Handle pointer motion: update position, drag, and hover target.
    pub fn on_pointer_motion(&mut self, x: f64, y: f64) {
        self.pointer_location = (x, y).into();
        let point = (x.round() as i32, y.round() as i32);

        // An active move consumes motion; it does not hover.
        if self.windows.drag().is_some() {
            self.windows.drag_to(point, work_area(self.output_size));
            return;
        }

        let focus = self
            .windows
            .hit_test(point.0, point.1)
            .and_then(|id| self.toplevel_for(id))
            .map(|t| t.wl_surface().clone());
        let serial = SERIAL_COUNTER.next_serial();
        let event = MotionEvent {
            location: (x, y).into(),
            serial,
            time: self.now_ms(),
        };
        let pointer = self.pointer.clone();
        pointer.motion(self, focus.map(|s| (s, (x, y).into())), &event);
        pointer.frame(self);
    }

    /// Handle a pointer button press or release.
    ///
    /// Press raises and focuses the window under the cursor, then forwards
    /// the event. Release ends any interactive move.
    ///
    /// `super_down` + left press starts a compositor-initiated move, the
    /// convention on Linux desktops.
    pub fn on_pointer_button(&mut self, button: u32, state: ButtonState, super_down: bool) {
        let point = (
            self.pointer_location.x.round() as i32,
            self.pointer_location.y.round() as i32,
        );
        self.stats.buttons += 1;

        match state {
            ButtonState::Pressed => {
                if let Some(id) = self.windows.hit_test(point.0, point.1) {
                    self.windows.activate(id);
                    if let Some(t) = self.toplevel_for(id) {
                        self.set_keyboard_focus(Some(t.wl_surface().clone()));
                    }
                    // BTN_LEFT (0x110) with Super starts a compositor move.
                    if button == 0x110 && super_down && self.windows.begin_move(id, point).is_some()
                    {
                        self.stats.moves += 1;
                    }
                } else {
                    self.set_keyboard_focus(None);
                }
            }
            ButtonState::Released => {
                self.windows.end_move();
            }
        }

        let event = ButtonEvent {
            serial: SERIAL_COUNTER.next_serial(),
            time: self.now_ms(),
            button,
            state,
        };
        let pointer = self.pointer.clone();
        pointer.button(self, &event);
        pointer.frame(self);
    }

    /// Deliver a key event to the focused surface.
    pub fn on_keyboard_key(&mut self, keycode: Keycode, state: KeyState) {
        self.stats.keys += 1;
        let keyboard = self.keyboard.clone();
        let time = self.now_ms();
        let _ = keyboard.input::<(), _>(
            self,
            keycode,
            state,
            SERIAL_COUNTER.next_serial(),
            time,
            |_, _, _| smithay::input::keyboard::FilterResult::Forward,
        );
    }

    /// Queue frame callbacks on every live surface.
    ///
    /// Clients that render on `wl_surface.frame` will not draw a second
    /// frame until this fires, so a backend must call it exactly once per
    /// composited frame. Returns how many callbacks it sent.
    pub fn send_frame_callbacks(&mut self) -> u64 {
        let time = self.now_ms();
        let surfaces: Vec<WlSurface> = self
            .toplevels
            .values()
            .map(|t| t.wl_surface().clone())
            .chain(self.layers.iter().map(|l| l.surface.wl_surface().clone()))
            .collect();
        let mut sent = 0;
        for surface in &surfaces {
            sent += send_frames_surface_tree(surface, time);
        }
        self.stats.frames_sent += sent;
        sent
    }

    /// Number of layer surfaces currently alive.
    #[must_use]
    pub fn layer_surface_count(&self) -> usize {
        self.layers.len()
    }

    /// Everything the compositor can tell a human about itself.
    #[must_use]
    pub fn describe(&self) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        let area = work_area(self.output_size);
        let _ = writeln!(
            out,
            "output={}x{} work_area={}x{} panel_height={PANEL_HEIGHT}",
            self.output_size.w, self.output_size.h, area.w, area.h
        );
        let _ = writeln!(
            out,
            "windows={} focused={:?}",
            self.windows.len(),
            self.windows.focused_id()
        );
        for w in self.windows.windows() {
            let _ = writeln!(
                out,
                "  {} rect={},{} {}x{} mapped={} app={:?} title={:?}",
                w.id, w.rect.x, w.rect.y, w.rect.w, w.rect.h, w.mapped, w.app_id, w.title
            );
        }
        for l in &self.layers {
            let _ = writeln!(
                out,
                "  layer {} ns={:?} size={}x{}",
                format!("{:?}", l.layer).to_lowercase(),
                l.namespace,
                l.size.w,
                l.size.h
            );
        }
        let _ = writeln!(out, "stats={:?}", self.stats);
        out
    }
}

/// Drain and count frame callbacks for one surface tree.
pub fn send_frames_surface_tree(surface: &WlSurface, time: u32) -> u64 {
    let mut sent = 0u64;
    with_surface_tree_downward(
        surface,
        (),
        |_, _, &()| TraversalAction::DoChildren(()),
        |_, states, &()| {
            for callback in states
                .cached_state
                .get::<SurfaceAttributes>()
                .current()
                .frame_callbacks
                .drain(..)
            {
                callback.done(time);
                sent += 1;
            }
        },
        |_, _, &()| true,
    );
    sent
}

/// Logical `(width, height)` of the buffer a surface currently holds.
///
/// Must be called **after** [`on_commit_buffer_handler`], which is what
/// moves the pending buffer into Smithay's renderer state — the raw
/// `SurfaceAttributes::buffer` field is `take()`n in the process, so
/// reading it directly always yields `None` on the frame that matters.
/// `buffer_size()` also applies buffer scale and transform for us.
#[must_use]
pub fn surface_buffer_size(surface: &WlSurface) -> Option<(i32, i32)> {
    with_states(surface, |states| {
        let data = states.data_map.get::<RendererSurfaceStateUserData>()?;
        let guard = data.lock().ok()?;
        guard.buffer_size().map(|s| (s.w, s.h))
    })
}

// ---------------------------------------------------------------------------
// Protocol handlers
// ---------------------------------------------------------------------------

impl BufferHandler for Tahoe {
    fn buffer_destroyed(&mut self, _buffer: &wl_buffer::WlBuffer) {}
}

impl CompositorHandler for Tahoe {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client.get_data::<ClientState>().unwrap().compositor_state
    }

    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Tahoe>(surface);
        self.stats.commits += 1;

        let Some(id) = self.window_id_for(surface) else {
            return;
        };
        // A commit with no buffer is the client asking for its initial
        // configure; it must not make the window "mapped".
        let Some((w, h)) = surface_buffer_size(surface) else {
            return;
        };
        if self.windows.on_buffer_attached(id, (w, h)) {
            self.stats.first_commits += 1;
            // First real frame: the client is now drawable, so it is safe
            // to raise and focus it.
            self.windows.activate(id);
            if let Some(t) = self.toplevel_for(id) {
                let s = t.wl_surface().clone();
                self.set_keyboard_focus(Some(s));
            }
        }
    }
}

impl ShmHandler for Tahoe {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

impl SeatHandler for Tahoe {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }

    fn focus_changed(&mut self, _seat: &Seat<Self>, focused: Option<&WlSurface>) {
        // Keep the window stack in sync when Smithay changes focus itself,
        // for example when a client disconnects while focused.
        let id = focused.and_then(|s| self.window_id_for(s));
        self.windows.set_focused(id);
    }

    fn cursor_image(
        &mut self,
        _seat: &Seat<Self>,
        _image: smithay::input::pointer::CursorImageStatus,
    ) {
        // Cursor rendering belongs to the backend: the winit backend shows
        // the host cursor, the TTY backend will need a cursor surface.
    }
}

impl OutputHandler for Tahoe {
    fn output_bound(&mut self, _output: Output, _wl_output: WlOutput) {}
}

impl SelectionHandler for Tahoe {
    type SelectionUserData = ();
}

impl DataDeviceHandler for Tahoe {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}

impl ClientDndGrabHandler for Tahoe {}
impl ServerDndGrabHandler for Tahoe {
    fn send(&mut self, _mime_type: String, _fd: OwnedFd, _seat: Seat<Self>) {}
}

impl XdgShellHandler for Tahoe {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let suggested = self.suggested_size();
        let area = work_area(self.output_size);
        let id = self
            .windows
            .insert(area, (suggested.w, suggested.h), self.cascade);
        self.toplevels.insert(id, surface.clone());
        self.stats.toplevels_created += 1;

        surface.with_pending_state(|state| {
            state.states.set(xdg_toplevel::State::Activated);
            state.size = Some(suggested);
        });
        surface.send_configure();
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        if let Some((id, _)) = self
            .toplevels
            .iter()
            .find(|(_, t)| t.wl_surface() == surface.wl_surface())
        {
            let id = *id;
            self.toplevels.remove(&id);
            self.windows.remove(id);
        }
        self.stats.toplevels_destroyed += 1;
    }

    fn move_request(&mut self, surface: ToplevelSurface, _seat: wl_seat::WlSeat, _serial: Serial) {
        // Client-initiated move (titlebar drag). This is the well-behaved
        // path and does not require the Super modifier.
        if let Some(id) = self.window_id_for(surface.wl_surface()) {
            let point = (
                self.pointer_location.x.round() as i32,
                self.pointer_location.y.round() as i32,
            );
            if self.windows.begin_move(id, point).is_some() {
                self.stats.moves += 1;
            }
        }
    }

    fn app_id_changed(&mut self, surface: ToplevelSurface) {
        let app_id = read_toplevel_attr(surface.wl_surface(), |a| a.app_id.clone()).flatten();
        if let Some(id) = self.window_id_for(surface.wl_surface()) {
            self.windows.set_metadata(id, None, app_id);
        }
    }

    fn title_changed(&mut self, surface: ToplevelSurface) {
        let title = read_toplevel_attr(surface.wl_surface(), |a| a.title.clone()).flatten();
        if let Some(id) = self.window_id_for(surface.wl_surface()) {
            self.windows.set_metadata(id, title, None);
        }
    }

    fn new_popup(&mut self, _surface: PopupSurface, _positioner: PositionerState) {}

    fn grab(&mut self, _surface: PopupSurface, _seat: wl_seat::WlSeat, _serial: Serial) {}

    fn reposition_request(
        &mut self,
        _surface: PopupSurface,
        _positioner: PositionerState,
        _token: u32,
    ) {
    }
}

/// Read a field out of a toplevel's role attributes, taking the lock
/// defensively so a poisoned lock cannot panic the compositor.
fn read_toplevel_attr<T>(
    surface: &WlSurface,
    f: impl FnOnce(&smithay::wayland::shell::xdg::XdgToplevelSurfaceRoleAttributes) -> T,
) -> Option<T> {
    with_states(surface, |states| {
        let data = states.data_map.get::<XdgToplevelSurfaceData>()?;
        let guard = data.lock().ok()?;
        Some(f(&guard))
    })
}

impl WlrLayerShellHandler for Tahoe {
    fn shell_state(&mut self) -> &mut WlrLayerShellState {
        &mut self.layer_shell_state
    }

    fn new_layer_surface(
        &mut self,
        surface: LayerSurface,
        _output: Option<WlOutput>,
        layer: Layer,
        namespace: String,
    ) {
        let (anchor, requested) = with_states(surface.wl_surface(), |states| {
            let mut cached = states.cached_state.get::<LayerSurfaceCachedState>();
            let current = cached.current();
            (current.anchor, current.size)
        });

        // A surface anchored to both horizontal edges spans the output
        // width; likewise vertically. Otherwise the client's request wins.
        let mut size = Size::from((0, 0));
        size.w = if anchor.anchored_horizontally() {
            self.output_size.w
        } else {
            requested.w.max(0)
        };
        size.h = if anchor.anchored_vertically() {
            self.output_size.h
        } else {
            requested.h.max(0)
        };

        surface.with_pending_state(|state| {
            state.size = Some(size);
        });
        surface.send_configure();

        self.layers.push(ManagedLayerSurface {
            surface,
            namespace: namespace.clone(),
            layer,
            size,
        });
        self.stats.layer_surfaces_created += 1;
        tracing::info!(
            namespace,
            ?layer,
            w = size.w,
            h = size.h,
            "new layer surface"
        );
    }

    fn layer_destroyed(&mut self, surface: LayerSurface) {
        self.layers
            .retain(|l| l.surface.wl_surface() != surface.wl_surface());
        self.stats.layer_surfaces_destroyed += 1;
        tracing::info!("layer surface destroyed");
    }
}

/// Per-client data. Required by Smithay for every inserted client.
#[derive(Default)]
pub struct ClientState {
    /// Compositor-side per-client state Smithay needs to track.
    pub compositor_state: CompositorClientState,
    counters: Option<Arc<ClientCounters>>,
}

impl ClientState {
    /// Build per-client data that reports its own disconnect to `counters`.
    #[must_use]
    pub fn with_counters(counters: Arc<ClientCounters>) -> Self {
        Self {
            compositor_state: CompositorClientState::default(),
            counters: Some(counters),
        }
    }
}

impl smithay::reexports::wayland_server::backend::ClientData for ClientState {
    fn initialized(&self, _client_id: smithay::reexports::wayland_server::backend::ClientId) {
        tracing::debug!("client initialized");
    }

    fn disconnected(
        &self,
        _client_id: smithay::reexports::wayland_server::backend::ClientId,
        reason: smithay::reexports::wayland_server::backend::DisconnectReason,
    ) {
        if let Some(counters) = &self.counters {
            counters.count_disconnected();
        }
        tracing::debug!(?reason, "client disconnected");
    }
}

impl Drop for Tahoe {
    fn drop(&mut self) {
        tracing::info!(
            windows = self.windows.len(),
            layers = self.layers.len(),
            "compositor shutting down"
        );
    }
}

/// Geometry helper shared with tests and the renderer.
#[must_use]
pub fn window_rectangle(rect: Rect) -> Rectangle<i32, Logical> {
    Rectangle::new((rect.x, rect.y).into(), (rect.w, rect.h).into())
}

delegate_compositor!(Tahoe);
delegate_shm!(Tahoe);
delegate_seat!(Tahoe);
delegate_data_device!(Tahoe);
delegate_xdg_shell!(Tahoe);
delegate_layer_shell!(Tahoe);
delegate_output!(Tahoe);

/// Delegate macros for output live in Smithay's `delegate_output!`; kept in
/// one place so the protocol surface is auditable at a glance.
#[cfg(test)]
mod tests;
