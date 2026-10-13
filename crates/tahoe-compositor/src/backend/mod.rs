// SPDX-License-Identifier: MIT OR Apache-2.0
//! Backends: how a frame reaches a screen, and how input arrives.
//!
//! Both backends drive the same [`Tahoe`](crate::state::Tahoe) state and
//! the same protocol surface; they differ only in where pixels and events
//! come from. Keeping the protocol layer backend-independent is what makes
//! it possible to test the compositor's real behaviour headlessly.

pub mod headless;

#[cfg(feature = "winit")]
pub mod winit;

use std::path::{Path, PathBuf};

use smithay::reexports::wayland_server::ListeningSocket;

/// Where a compositor's Wayland socket lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocketSpec {
    /// A name inside `XDG_RUNTIME_DIR`, as a real session uses.
    Named(String),
    /// An absolute path. Used by tests so they never touch the user's
    /// runtime directory or an existing session's socket.
    Path(PathBuf),
}

impl SocketSpec {
    /// Bind the socket.
    ///
    /// # Errors
    ///
    /// Returns a human-readable error when the path is already in use, the
    /// runtime directory is unset, or the filesystem refuses the create.
    pub fn bind(&self) -> Result<ListeningSocket, String> {
        match self {
            Self::Named(name) => {
                ListeningSocket::bind(name).map_err(|e| format!("bind {name}: {e}"))
            }
            Self::Path(path) => ListeningSocket::bind_absolute(path.clone())
                .map_err(|e| format!("bind {}: {e}", path.display())),
        }
    }
}

/// A socket path unique to this process and tag, inside `dir`.
///
/// Tests run concurrently in one binary, so a fixed name would collide. The
/// process id plus the tag makes the name unique per test, and any stale
/// socket or lock file from a previous crashed run is removed first —
/// `ListeningSocket` refuses to bind over an existing name.
///
/// # Errors
///
/// Returns an error when `dir` cannot be created.
pub fn unique_socket_path(dir: &Path, tag: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("tahoe-{tag}-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("lock"));
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> PathBuf {
        std::env::temp_dir().join("tahoe-backend-test")
    }

    #[test]
    fn unique_path_is_absolute_and_tagged() {
        let p = unique_socket_path(&dir(), "tag").expect("path");
        assert!(p.is_absolute());
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.contains("tag"), "{name}");
        assert!(name.ends_with(".sock"), "{name}");
        assert!(name.contains(&std::process::id().to_string()), "{name}");
    }

    #[test]
    fn unique_path_is_stable_for_same_inputs() {
        assert_eq!(
            unique_socket_path(&dir(), "same").unwrap(),
            unique_socket_path(&dir(), "same").unwrap()
        );
    }

    #[test]
    fn unique_path_clears_stale_files() {
        let p = unique_socket_path(&dir(), "stale").unwrap();
        std::fs::write(&p, b"stale").unwrap();
        let p2 = unique_socket_path(&dir(), "stale").unwrap();
        assert_eq!(p, p2);
        assert!(!p.exists(), "stale socket must be removed before binding");
    }

    #[test]
    fn different_tags_do_not_collide() {
        assert_ne!(
            unique_socket_path(&dir(), "a").unwrap(),
            unique_socket_path(&dir(), "b").unwrap()
        );
    }
}
