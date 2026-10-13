// SPDX-License-Identifier: MIT OR Apache-2.0
//! Headless backend: a real Wayland compositor with no display attached.
//!
//! This is not a stub. It registers the same globals as the windowed
//! backend and serves real clients over a real Unix socket; it simply never
//! renders, because there is nothing to render to. That makes it the right
//! target for two things:
//!
//! 1. **The test suite** — `tests/wayland_client.rs` drives a genuine
//!    `wayland-client` connection against it and asserts protocol-level
//!    behaviour (configure, commit, focus, lifecycle).
//! 2. **CI and diagnostics** — it runs with no GPU, no DRM node and no seat.
//!
//! What it deliberately does *not* do: composite pixels. There is no
//! renderer here, so `--backend headless` proves the protocol and window
//! model, not the drawing path.

use std::{
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use smithay::reexports::wayland_server::{Display, ListeningSocket};
use smithay::utils::{Logical, Size};

use super::SocketSpec;
use crate::state::{ClientState, Tahoe};

/// How long the pump sleeps between non-blocking dispatches.
///
/// `Display::dispatch_clients` is non-blocking (the backend polls epoll
/// with a zero timeout), so the pump owns the pacing. One millisecond keeps
/// tests fast without spinning a core.
const PUMP_INTERVAL: Duration = Duration::from_millis(1);

/// A compositor that serves clients without a display.
pub struct HeadlessCompositor {
    /// Protocol and window state, shared with the other backends.
    pub state: Tahoe,
    display: Display<Tahoe>,
    listener: ListeningSocket,
    socket_path: Option<PathBuf>,
}

impl HeadlessCompositor {
    /// Bind a socket and build the state. Nothing is served until
    /// [`HeadlessCompositor::dispatch`] or a pump runs.
    ///
    /// # Errors
    ///
    /// Returns an error when the Wayland `Display` cannot be created or the
    /// socket cannot be bound.
    pub fn bind(socket: SocketSpec, output_size: Size<i32, Logical>) -> Result<Self, String> {
        let listener = socket.bind()?;
        let display: Display<Tahoe> = Display::new().map_err(|e| format!("Display::new: {e}"))?;
        let state = Tahoe::new(&display.handle(), output_size);
        let socket_path = match &socket {
            SocketSpec::Path(p) => Some(p.clone()),
            SocketSpec::Named(_) => None,
        };
        Ok(Self {
            state,
            display,
            listener,
            socket_path,
        })
    }

    /// Path of the listening socket, when it is a filesystem path.
    #[must_use]
    pub fn socket_path(&self) -> Option<&Path> {
        self.socket_path.as_deref()
    }

    /// Accept pending connections, dispatch every queued request, and flush
    /// replies. Also fires one round of frame callbacks, exactly as a
    /// rendering backend would per composited frame.
    ///
    /// # Errors
    ///
    /// Returns an error only on a fatal I/O failure; a client vanishing
    /// mid-dispatch is normal and handled by Smithay.
    pub fn dispatch(&mut self) -> std::io::Result<()> {
        while let Some(stream) = self.listener.accept()? {
            self.accept_client(stream)?;
        }
        self.display.dispatch_clients(&mut self.state)?;
        self.display.flush_clients()?;
        self.state.send_frame_callbacks();
        self.display.flush_clients()?;
        Ok(())
    }

    fn accept_client(&mut self, stream: UnixStream) -> std::io::Result<()> {
        let data = Arc::new(ClientState::with_counters(self.state.client_counters()));
        let client = self.display.handle().insert_client(stream, data)?;
        self.state.count_client_connected();
        tracing::debug!(client = ?client.id(), "client connected");
        Ok(())
    }

    /// Clients currently connected.
    ///
    /// Derived rather than stored: wayland-server has no liveness query for
    /// a `ClientId`, and its `ClientData::disconnected` callback fires
    /// exactly once per accepted connection, so the difference is exact.
    #[must_use]
    pub fn client_count(&self) -> u64 {
        self.state
            .clients_connected()
            .saturating_sub(self.state.clients_disconnected())
    }

    /// Run the pump until `done` returns true or `timeout` elapses.
    ///
    /// Returns `Ok(true)` when `done` fired, `Ok(false)` on timeout. Used by
    /// tests instead of sleeping for a guessed duration.
    ///
    /// # Errors
    ///
    /// Propagates fatal dispatch errors.
    pub fn pump_until(
        &mut self,
        timeout: Duration,
        mut done: impl FnMut(&Tahoe) -> bool,
    ) -> std::io::Result<bool> {
        let deadline = Instant::now() + timeout;
        loop {
            self.dispatch()?;
            if done(&self.state) {
                return Ok(true);
            }
            if Instant::now() >= deadline {
                return Ok(false);
            }
            std::thread::sleep(PUMP_INTERVAL);
        }
    }

    /// Run the pump for a fixed duration. Useful when a test wants the
    /// compositor to observe something for a while.
    ///
    /// # Errors
    ///
    /// Propagates fatal dispatch errors.
    pub fn pump_for(&mut self, duration: Duration) -> std::io::Result<()> {
        let deadline = Instant::now() + duration;
        while Instant::now() < deadline {
            self.dispatch()?;
            std::thread::sleep(PUMP_INTERVAL);
        }
        Ok(())
    }

    /// Serve until `should_stop` returns true. This is what the `headless`
    /// CLI mode uses.
    ///
    /// # Errors
    ///
    /// Propagates fatal dispatch errors.
    pub fn run(&mut self, mut should_stop: impl FnMut(&Tahoe) -> bool) -> std::io::Result<()> {
        while !should_stop(&self.state) {
            self.dispatch()?;
            std::thread::sleep(PUMP_INTERVAL);
        }
        Ok(())
    }
}

impl Drop for HeadlessCompositor {
    fn drop(&mut self) {
        if let Some(path) = &self.socket_path {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::unique_socket_path;

    fn socket(tag: &str) -> SocketSpec {
        let dir = std::env::temp_dir().join("tahoe-headless-test");
        SocketSpec::Path(unique_socket_path(&dir, tag).expect("socket path"))
    }

    #[test]
    fn binds_a_socket_and_reports_its_path() {
        let spec = socket("bind");
        let comp = HeadlessCompositor::bind(spec.clone(), (1280, 720).into()).expect("bind");
        let path = comp.socket_path().expect("path").to_path_buf();
        assert!(
            path.exists(),
            "socket file should exist at {}",
            path.display()
        );
        drop(comp);
        assert!(!path.exists(), "socket must be cleaned up on drop");
    }

    #[test]
    fn dispatches_with_no_clients() {
        let mut comp = HeadlessCompositor::bind(socket("idle"), (800, 600).into()).expect("bind");
        comp.pump_for(Duration::from_millis(20)).expect("pump");
        assert_eq!(comp.client_count(), 0);
        assert_eq!(comp.state.clients_connected(), 0);
        assert_eq!(comp.client_count(), 0);
    }

    #[test]
    fn pump_until_times_out_honestly() {
        let mut comp =
            HeadlessCompositor::bind(socket("timeout"), (800, 600).into()).expect("bind");
        let fired = comp
            .pump_until(Duration::from_millis(30), |_| false)
            .expect("pump");
        assert!(!fired, "a condition that never holds must report a timeout");
    }

    #[test]
    fn pump_until_returns_true_when_condition_holds() {
        let mut comp = HeadlessCompositor::bind(socket("cond"), (800, 600).into()).expect("bind");
        let fired = comp
            .pump_until(Duration::from_millis(200), |_| true)
            .expect("pump");
        assert!(fired);
    }

    #[test]
    fn second_bind_on_same_path_fails_cleanly() {
        let spec = socket("dup");
        let _first = HeadlessCompositor::bind(spec.clone(), (640, 480).into()).expect("first bind");
        let second = HeadlessCompositor::bind(spec, (640, 480).into());
        assert!(
            second.is_err(),
            "binding a live socket must fail, not corrupt it"
        );
    }

    #[test]
    fn work_area_excludes_the_panel() {
        let comp = HeadlessCompositor::bind(socket("area"), (1000, 700).into()).expect("bind");
        let area = crate::state::work_area(comp.state.output_size);
        assert_eq!(area.y, crate::PANEL_HEIGHT);
        assert_eq!(area.h, 700 - crate::PANEL_HEIGHT);
    }
}
