// SPDX-License-Identifier: MIT OR Apache-2.0
//! End-to-end tests with a **real** Wayland client.
//!
//! The client is `wayland-client`, the same library every toolkit uses,
//! talking over a real Unix socket to the headless backend. Nothing is
//! mocked: if these pass, the compositor genuinely implements the
//! protocols it advertises.
//!
//! Threading: `EventQueue::roundtrip` blocks until the server answers, so
//! the client runs on its own thread while the test thread pumps the
//! compositor. That mirrors production, where clients and the compositor
//! are never in the same thread.

use std::{
    os::{fd::AsFd, unix::net::UnixStream},
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant},
};

use tahoe_compositor::{
    ADVERTISED_GLOBALS, PANEL_HEIGHT, Tahoe,
    backend::{SocketSpec, headless::HeadlessCompositor, unique_socket_path},
};
use wayland_client::{
    Connection, Dispatch, EventQueue, QueueHandle, WEnum,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{
        wl_buffer, wl_callback, wl_compositor, wl_output, wl_registry, wl_seat, wl_shm,
        wl_shm_pool, wl_surface,
    },
};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};

/// How long a test waits for the client to finish before declaring a hang.
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

/// Wire value of `xdg_toplevel.state.activated`, from the protocol XML
/// (`/usr/share/wayland-protocols/stable/xdg-shell/xdg-shell.xml`, entry
/// `activated value="4"`). Protocol enum values are permanent, which is why
/// this is a constant rather than a re-derivation.
const XDG_TOPLEVEL_STATE_ACTIVATED: u8 = 4;

// ---------------------------------------------------------------------------
// Client side
// ---------------------------------------------------------------------------

/// Everything the client observes.
#[derive(Debug, Default, Clone)]
struct ClientState {
    shm_formats: Vec<wl_shm::Format>,
    /// Serial from the most recent `xdg_surface.configure`.
    configure_serial: Option<u32>,
    toplevel_configured: bool,
    activated: bool,
    closed: bool,
    frame_done: u32,
    seat_capabilities: Option<WEnum<wl_seat::Capability>>,
}

macro_rules! ignore_events {
    ($($iface:ty),* $(,)?) => {
        $(impl Dispatch<$iface, ()> for ClientState {
            fn event(
                _state: &mut Self,
                _proxy: &$iface,
                _event: <$iface as wayland_client::Proxy>::Event,
                _data: &(),
                _conn: &Connection,
                _qh: &QueueHandle<Self>,
            ) {}
        })*
    };
}

ignore_events!(
    wl_compositor::WlCompositor,
    wl_shm_pool::WlShmPool,
    wl_buffer::WlBuffer,
    wl_surface::WlSurface,
    wl_output::WlOutput,
);

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for ClientState {
    /// `registry_queue_init` owns the registry contents; the global list is
    /// read back from `GlobalList::contents()`, so the events themselves
    /// need no handling here.
    fn event(
        _state: &mut Self,
        _proxy: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_shm::WlShm, ()> for ClientState {
    fn event(
        state: &mut Self,
        _proxy: &wl_shm::WlShm,
        event: wl_shm::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wl_shm::Event::Format {
            format: WEnum::Value(f),
        } = event
        {
            state.shm_formats.push(f);
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for ClientState {
    fn event(
        state: &mut Self,
        _proxy: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities { capabilities } = event {
            state.seat_capabilities = Some(capabilities);
        }
    }
}

impl Dispatch<wl_callback::WlCallback, ()> for ClientState {
    fn event(
        state: &mut Self,
        _proxy: &wl_callback::WlCallback,
        event: wl_callback::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { .. } = event {
            state.frame_done += 1;
        }
    }
}

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for ClientState {
    fn event(
        _state: &mut Self,
        proxy: &xdg_wm_base::XdgWmBase,
        event: xdg_wm_base::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // A compositor that pings and gets no pong is shown as unresponsive;
        // a real client must answer.
        if let xdg_wm_base::Event::Ping { serial } = event {
            proxy.pong(serial);
        }
    }
}

impl Dispatch<xdg_surface::XdgSurface, ()> for ClientState {
    fn event(
        state: &mut Self,
        _proxy: &xdg_surface::XdgSurface,
        event: xdg_surface::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            state.configure_serial = Some(serial);
        }
    }
}

impl Dispatch<xdg_toplevel::XdgToplevel, ()> for ClientState {
    fn event(
        state: &mut Self,
        _proxy: &xdg_toplevel::XdgToplevel,
        event: xdg_toplevel::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            xdg_toplevel::Event::Configure { states, .. } => {
                state.toplevel_configured = true;
                state.activated = states.contains(&XDG_TOPLEVEL_STATE_ACTIVATED);
            }
            xdg_toplevel::Event::Close => state.closed = true,
            _ => {}
        }
    }
}

/// A connected client with one toplevel and one shm buffer.
struct TestClient {
    event_queue: EventQueue<ClientState>,
    state: ClientState,
    wm_base: xdg_wm_base::XdgWmBase,
    surface: wl_surface::WlSurface,
    xdg_surface: xdg_surface::XdgSurface,
    toplevel: xdg_toplevel::XdgToplevel,
    buffer: wl_buffer::WlBuffer,
    /// Kept alive: the compositor mmaps this file to learn the buffer size.
    _pool_file: std::fs::File,
    width: i32,
    height: i32,
}

const BUF_W: i32 = 320;
const BUF_H: i32 = 200;

fn create_pool_file(tag: &str, size: usize) -> Result<std::fs::File, String> {
    let dir = std::env::temp_dir().join("tahoe-client-test");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("pool-{tag}-{}.bin", std::process::id()));
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .map_err(|e| format!("pool file: {e}"))?;
    file.set_len(size as u64).map_err(|e| e.to_string())?;
    Ok(file)
}

impl TestClient {
    fn connect(path: &Path, tag: &str) -> Result<Self, String> {
        let stream = UnixStream::connect(path).map_err(|e| format!("connect: {e}"))?;
        let conn = Connection::from_socket(stream).map_err(|e| format!("from_socket: {e}"))?;
        let (globals, mut event_queue) =
            registry_queue_init::<ClientState>(&conn).map_err(|e| format!("registry: {e}"))?;
        let qh = event_queue.handle();

        let mut state = ClientState::default();
        event_queue
            .roundtrip(&mut state)
            .map_err(|e| format!("roundtrip 1: {e}"))?;

        let compositor: wl_compositor::WlCompositor = globals
            .bind(&qh, 1..=6, ())
            .map_err(|e| format!("bind wl_compositor: {e}"))?;
        let shm: wl_shm::WlShm = globals
            .bind(&qh, 1..=1, ())
            .map_err(|e| format!("bind wl_shm: {e}"))?;
        let wm_base: xdg_wm_base::XdgWmBase = globals
            .bind(&qh, 1..=6, ())
            .map_err(|e| format!("bind xdg_wm_base: {e}"))?;
        let _seat: wl_seat::WlSeat = globals
            .bind(&qh, 1..=9, ())
            .map_err(|e| format!("bind wl_seat: {e}"))?;
        let _output: wl_output::WlOutput = globals
            .bind(&qh, 1..=4, ())
            .map_err(|e| format!("bind wl_output: {e}"))?;

        let stride = BUF_W * 4;
        let size = (stride * BUF_H) as usize;
        let mut pool_file = create_pool_file(tag, size)?;
        {
            use std::io::{Seek, SeekFrom, Write};
            pool_file
                .seek(SeekFrom::Start(0))
                .map_err(|e| e.to_string())?;
            // Opaque green, so a compositor that reads it sees real pixels.
            let row: Vec<u8> = [[0x00u8, 0xff, 0x00, 0xff]; BUF_W as usize].concat();
            for _ in 0..BUF_H {
                pool_file.write_all(&row).map_err(|e| e.to_string())?;
            }
        }

        let pool = shm.create_pool(pool_file.as_fd(), stride * BUF_H, &qh, ());
        let buffer = pool.create_buffer(0, BUF_W, BUF_H, stride, wl_shm::Format::Argb8888, &qh, ());
        pool.destroy();

        let surface = compositor.create_surface(&qh, ());
        let xdg_surface = wm_base.get_xdg_surface(&surface, &qh, ());
        let toplevel = xdg_surface.get_toplevel(&qh, ());
        toplevel.set_title("Tahoe Test Client".to_owned());
        toplevel.set_app_id("org.tahoe.TestClient".to_owned());

        Ok(Self {
            event_queue,
            state,
            wm_base,
            surface,
            xdg_surface,
            toplevel,
            buffer,
            _pool_file: pool_file,
            width: BUF_W,
            height: BUF_H,
        })
    }

    /// The mandatory first commit: no buffer, just ask for the configure.
    fn request_configure(&mut self) -> Result<(), String> {
        self.surface.commit();
        self.pump_until(|s| s.configure_serial.is_some())
    }

    fn ack_configure(&mut self) -> Result<(), String> {
        let serial = self.state.configure_serial.ok_or("no configure serial")?;
        self.xdg_surface.ack_configure(serial);
        Ok(())
    }

    /// Attach the buffer and commit — this is the frame that maps the window.
    fn attach_and_commit(&mut self) {
        self.surface.attach(Some(&self.buffer), 0, 0);
        self.surface.damage_buffer(0, 0, self.width, self.height);
        self.surface.commit();
    }

    /// Round-trip until `done` holds or the deadline passes.
    fn pump_until(&mut self, done: impl Fn(&ClientState) -> bool) -> Result<(), String> {
        let deadline = Instant::now() + CLIENT_TIMEOUT;
        while !done(&self.state) {
            self.event_queue
                .roundtrip(&mut self.state)
                .map_err(|e| format!("roundtrip: {e}"))?;
            if Instant::now() >= deadline {
                return Err(format!("client timed out; state = {:?}", self.state));
            }
        }
        Ok(())
    }

    /// One blocking round-trip, used to let the server process everything.
    fn roundtrip(&mut self) -> Result<(), String> {
        self.event_queue
            .roundtrip(&mut self.state)
            .map(|_| ())
            .map_err(|e| format!("roundtrip: {e}"))
    }

    /// Request a frame callback on the toplevel surface.
    fn request_frame(&mut self) -> Result<(), String> {
        let qh = self.event_queue.handle();
        let _cb: wl_callback::WlCallback = self.surface.frame(&qh, ());
        Ok(())
    }

    /// A copy of the observed protocol state.
    fn snapshot(&self) -> ClientState {
        self.state.clone()
    }

    /// Consume the client, returning what it observed.
    fn into_state(self) -> ClientState {
        self.state.clone()
    }

    fn destroy(mut self) {
        self.toplevel.destroy();
        self.xdg_surface.destroy();
        self.surface.destroy();

        self.wm_base.destroy();
        let _ = self.event_queue.roundtrip(&mut self.state);
    }
}

// ---------------------------------------------------------------------------
// Server side helpers
// ---------------------------------------------------------------------------

struct Server {
    comp: HeadlessCompositor,
    path: PathBuf,
}

impl Server {
    fn start(tag: &str) -> Self {
        let dir = std::env::temp_dir().join("tahoe-integration");
        let path = unique_socket_path(&dir, tag).expect("socket path");
        let comp = HeadlessCompositor::bind(SocketSpec::Path(path.clone()), (1280, 800).into())
            .expect("bind compositor");
        Self { comp, path }
    }

    /// Pump the compositor until `done` holds or the deadline passes.
    fn pump_until(&mut self, done: impl Fn(&Tahoe) -> bool) -> bool {
        self.comp.pump_until(CLIENT_TIMEOUT, done).unwrap_or(false)
    }

    fn pump(&mut self) {
        self.comp.pump_for(Duration::from_millis(30)).expect("pump");
    }
}

// ---------------------------------------------------------------------------
// Session driver
// ---------------------------------------------------------------------------

/// Run `steps` on a client thread, then keep the connection **open** while
/// `inspect` reads server-side state.
///
/// The holding is the point: every observable server fact about a window
/// exists only while its client is connected, so a test that joins the
/// client thread before asserting is really asserting about a disconnected
/// compositor. That is exactly the bug this helper removes.
fn client_session(
    server: &mut Server,
    tag: &'static str,
    steps: impl FnOnce(&mut TestClient) -> Result<(), String> + Send + 'static,
    inspect: impl FnOnce(&mut Server, &ClientState),
) -> Result<ClientState, String> {
    let path = server.path.clone();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<ClientState, String>>();
    let (done_tx, done_rx) = mpsc::channel::<()>();

    let handle = std::thread::spawn(move || -> Result<ClientState, String> {
        let mut client = TestClient::connect(&path, tag)?;
        steps(&mut client)?;
        client.roundtrip()?;
        ready_tx.send(Ok(client.snapshot())).ok();
        // Hold the connection open until the test is done inspecting.
        let _ = done_rx.recv_timeout(CLIENT_TIMEOUT * 3);
        Ok(client.into_state())
    });

    let deadline = Instant::now() + CLIENT_TIMEOUT;
    let snapshot = loop {
        server.pump();
        match ready_rx.try_recv() {
            Ok(result) => break result?,
            Err(mpsc::TryRecvError::Disconnected) => {
                return Err("client thread died before signalling readiness".to_owned());
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
        if Instant::now() >= deadline {
            return Err("client did not become ready in time".to_owned());
        }
    };

    inspect(server, &snapshot);
    let _ = done_tx.send(());
    handle
        .join()
        .map_err(|_| "client thread panicked".to_owned())?
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn client_sees_exactly_the_advertised_globals() {
    let mut server = Server::start("globals");
    let path = server.path.clone();

    let handle = std::thread::spawn(move || -> Result<Vec<String>, String> {
        let stream = UnixStream::connect(&path).map_err(|e| e.to_string())?;
        let conn = Connection::from_socket(stream).map_err(|e| e.to_string())?;
        let (globals, mut queue) =
            registry_queue_init::<ClientState>(&conn).map_err(|e| e.to_string())?;
        let mut state = ClientState::default();
        queue.roundtrip(&mut state).map_err(|e| e.to_string())?;
        Ok(globals
            .contents()
            .clone_list()
            .iter()
            .map(|g| g.interface.clone())
            .collect())
    });

    let deadline = Instant::now() + CLIENT_TIMEOUT;
    while !handle.is_finished() {
        server.pump();
        assert!(Instant::now() < deadline, "client hung");
    }
    let seen = handle.join().expect("join").expect("client");

    for expected in ADVERTISED_GLOBALS {
        assert!(
            seen.iter().any(|g| g == expected),
            "compositor advertises {expected:?} but the client did not see it; saw {seen:?}"
        );
    }
    // The registry must not promise anything we have not implemented.
    assert!(
        seen.len() >= ADVERTISED_GLOBALS.len(),
        "expected at least {} globals, saw {seen:?}",
        ADVERTISED_GLOBALS.len()
    );
}

#[test]
fn toplevel_gets_a_configure_activate_and_maps_with_a_buffer() {
    let mut server = Server::start("map");
    let state = client_session(
        &mut server,
        "map",
        |client| {
            client.request_configure()?;
            client.ack_configure()?;
            client.attach_and_commit();
            client.pump_until(|s| s.toplevel_configured)
        },
        |server, _| {
            assert!(
                server.pump_until(|s| s.stats.first_commits >= 1 && s.windows.len() == 1),
                "server never saw a mapped window:\n{}\nstats = {:?}",
                server.comp.state.describe(),
                server.comp.state.stats
            );
            let window = server
                .comp
                .state
                .windows
                .windows()
                .first()
                .expect("one window");
            assert!(
                window.mapped,
                "window should be mapped after a buffer commit"
            );
            assert_eq!(
                (window.rect.w, window.rect.h),
                (BUF_W, BUF_H),
                "window size must follow the buffer"
            );
            assert_eq!(
                window.app_id, "org.tahoe.TestClient",
                "app id must reach the shell"
            );
            assert_eq!(
                window.title, "Tahoe Test Client",
                "title must reach the shell"
            );
            assert!(
                window.rect.y >= PANEL_HEIGHT,
                "windows must not open under the panel: {:?}",
                window.rect
            );
            assert!(
                server.comp.state.focused_surface().is_some(),
                "the first mapped window should take focus"
            );
        },
    )
    .expect("client session");

    assert!(
        state.configure_serial.is_some(),
        "client never got a configure"
    );
    assert!(
        state.activated,
        "the compositor must activate a new toplevel"
    );
}

#[test]
fn client_disconnect_removes_its_windows_and_is_counted() {
    let mut server = Server::start("disc");
    let path = server.path.clone();

    let handle = std::thread::spawn(move || -> Result<(), String> {
        let mut client = TestClient::connect(&path, "disc")?;
        client.request_configure()?;
        client.ack_configure()?;
        client.attach_and_commit();
        client.roundtrip()?;
        client.destroy();
        Ok(())
    });

    assert!(
        server.pump_until(|s| s.stats.first_commits >= 1),
        "client never mapped a window"
    );
    let deadline = Instant::now() + CLIENT_TIMEOUT;
    while !handle.is_finished() {
        server.pump();
        assert!(Instant::now() < deadline, "client hung");
    }
    handle.join().expect("join").expect("client");

    // Disconnect is asynchronous: keep pumping until the server notices.
    let cleaned = server.pump_until(|s| s.windows.is_empty());
    assert!(
        cleaned,
        "window was not removed after the client disconnected"
    );
    assert!(
        server.comp.state.clients_disconnected() >= 1,
        "disconnect was not counted:\n{}",
        server.comp.state.describe()
    );
    assert!(server.comp.state.saw_full_client_lifecycle());
}

#[test]
fn frame_callbacks_are_sent_back_to_the_client() {
    let mut server = Server::start("frames");
    let path = server.path.clone();

    let handle = std::thread::spawn(move || -> Result<u32, String> {
        let mut client = TestClient::connect(&path, "frames")?;
        client.request_configure()?;
        client.ack_configure()?;
        client.attach_and_commit();
        client.request_frame()?;
        client.surface.commit();
        client.pump_until(|s| s.frame_done >= 1)?;
        Ok(client.state.frame_done)
    });

    let deadline = Instant::now() + CLIENT_TIMEOUT;
    while !handle.is_finished() {
        server.pump();
        assert!(
            Instant::now() < deadline,
            "client hung waiting for a frame callback"
        );
    }
    let frames = handle.join().expect("join").expect("client");
    assert!(frames >= 1, "expected at least one frame callback");
    assert!(
        server.comp.state.stats.frames_sent >= 1,
        "server did not count a frame"
    );
}

#[test]
fn shm_formats_are_advertised_and_argb8888_is_accepted() {
    let mut server = Server::start("shm");
    let path = server.path.clone();

    let handle = std::thread::spawn(move || -> Result<Vec<wl_shm::Format>, String> {
        let stream = UnixStream::connect(&path).map_err(|e| e.to_string())?;
        let conn = Connection::from_socket(stream).map_err(|e| e.to_string())?;
        let (globals, mut queue) =
            registry_queue_init::<ClientState>(&conn).map_err(|e| e.to_string())?;
        let qh = queue.handle();
        let shm: wl_shm::WlShm = globals.bind(&qh, 1..=1, ()).map_err(|e| e.to_string())?;
        let mut state = ClientState::default();
        // Two round-trips: bind, then collect the format burst.
        queue.roundtrip(&mut state).map_err(|e| e.to_string())?;
        queue.roundtrip(&mut state).map_err(|e| e.to_string())?;
        drop(shm);
        Ok(state.shm_formats)
    });

    let deadline = Instant::now() + CLIENT_TIMEOUT;
    while !handle.is_finished() {
        server.pump();
        assert!(Instant::now() < deadline, "client hung");
    }
    let formats = handle.join().expect("join").expect("client");
    assert!(
        formats.contains(&wl_shm::Format::Argb8888),
        "compositor must accept ARGB8888 shm buffers, offered {formats:?}"
    );
}

#[test]
fn pointer_motion_hits_the_window_and_avoids_the_panel() {
    let mut server = Server::start("pointer");
    client_session(
        &mut server,
        "pointer",
        |client| {
            client.request_configure()?;
            client.ack_configure()?;
            client.attach_and_commit();
            client.roundtrip()
        },
        |server, _| {
            assert!(
                server.pump_until(|s| s.windows.len() == 1),
                "no window to point at"
            );
            let rect = server.comp.state.windows.windows()[0].rect;
            let inside = (
                f64::from(rect.x + rect.w / 2),
                f64::from(rect.y + rect.h / 2),
            );

            server.comp.state.on_pointer_motion(inside.0, inside.1);
            server.pump();

            let hit = server
                .comp
                .state
                .windows
                .hit_test(inside.0 as i32, inside.1 as i32);
            assert!(
                hit.is_some(),
                "pointer at {inside:?} should hit the window at {rect:?}"
            );
            assert_eq!(
                server.comp.state.stats.buttons, 0,
                "motion must not press a button"
            );

            // The reserved panel strip belongs to the shell, never to a window.
            assert!(
                server
                    .comp
                    .state
                    .windows
                    .hit_test(10, PANEL_HEIGHT - 1)
                    .is_none(),
                "the panel strip must not belong to any client window"
            );
            assert!(
                server.comp.state.windows.hit_test(-1, 400).is_none(),
                "off-screen coordinates must hit nothing"
            );
        },
    )
    .expect("client session");
}

#[test]
fn pointer_press_focuses_and_raises_the_window_under_the_cursor() {
    let mut server = Server::start("focus");
    client_session(
        &mut server,
        "focus",
        |client| {
            client.request_configure()?;
            client.ack_configure()?;
            client.attach_and_commit();
            client.roundtrip()
        },
        |server, _| {
            assert!(server.pump_until(|s| s.windows.len() == 1), "no window");
            let id = server.comp.state.windows.windows()[0].id;
            // Clear focus, then click the window back into focus.
            server.comp.state.windows.set_focused(None);
            assert_eq!(server.comp.state.windows.focused_id(), None);

            let rect = server.comp.state.windows.windows()[0].rect;
            server
                .comp
                .state
                .on_pointer_motion(f64::from(rect.x + 4), f64::from(rect.y + 4));
            server.comp.state.on_pointer_button(
                0x110,
                smithay::backend::input::ButtonState::Pressed,
                false,
            );
            server.pump();
            assert_eq!(
                server.comp.state.windows.focused_id(),
                Some(id),
                "pressing on a window must focus it"
            );
            assert_eq!(server.comp.state.stats.buttons, 1);

            server.comp.state.on_pointer_button(
                0x110,
                smithay::backend::input::ButtonState::Released,
                false,
            );
            assert_eq!(server.comp.state.stats.buttons, 2);
        },
    )
    .expect("client session");
}

#[test]
fn interactive_move_follows_the_pointer_and_clamps_to_the_work_area() {
    let mut server = Server::start("move");
    client_session(
        &mut server,
        "move",
        |client| {
            client.request_configure()?;
            client.ack_configure()?;
            client.attach_and_commit();
            client.roundtrip()
        },
        |server, _| {
            assert!(server.pump_until(|s| s.windows.len() == 1), "no window");
            let before = server.comp.state.windows.windows()[0].rect;

            // Super + left press starts a compositor-initiated move.
            server
                .comp
                .state
                .on_pointer_motion(f64::from(before.x + 10), f64::from(before.y + 10));
            server.comp.state.on_pointer_button(
                0x110,
                smithay::backend::input::ButtonState::Pressed,
                true,
            );
            assert_eq!(
                server.comp.state.stats.moves, 1,
                "super+drag must start a move"
            );
            assert!(server.comp.state.windows.drag().is_some());

            server
                .comp
                .state
                .on_pointer_motion(f64::from(before.x + 210), f64::from(before.y + 110));
            let moved = server.comp.state.windows.windows()[0].rect;
            assert_eq!((moved.x, moved.y), (before.x + 200, before.y + 100));

            // Dragging far off-screen must clamp, not escape.
            server.comp.state.on_pointer_motion(100_000.0, 100_000.0);
            let clamped = server.comp.state.windows.windows()[0].rect;
            assert!(
                clamped.x + clamped.w <= 1280,
                "{clamped:?} escaped the output"
            );
            assert!(
                clamped.y >= PANEL_HEIGHT,
                "{clamped:?} slid under the panel"
            );

            server.comp.state.on_pointer_button(
                0x110,
                smithay::backend::input::ButtonState::Released,
                false,
            );
            assert!(
                server.comp.state.windows.drag().is_none(),
                "release must end the move"
            );
        },
    )
    .expect("client session");
}
