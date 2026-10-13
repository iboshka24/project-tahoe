// SPDX-License-Identifier: MIT OR Apache-2.0
//! `tahoe-shell`: the desktop shell's **state model**, without rendering.
//!
//! The model (dock, workspaces, launcher index) is a plain state machine:
//! unit-testable with no GPU, no Wayland socket, no toolkit. The future
//! view layer (smithay-client-toolkit, layer-shell surfaces) will render
//! snapshots of this model and forward events back into it.

use std::collections::BTreeMap;

/// An application identity: the basename of its `.desktop` file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AppId(pub String);

impl AppId {
    /// Create an id, trimming whitespace.
    #[must_use]
    pub fn new(id: &str) -> Self {
        Self(id.trim().to_owned())
    }
}

/// A window known to the shell. The compositor owns the surface; the shell
/// owns this bookkeeping record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowRecord {
    /// Compositor-assigned monotonically increasing id.
    pub id: u64,
    /// Human-readable title (may be empty until first commit).
    pub title: String,
    /// Owning application, when the compositor could determine it.
    pub app: Option<AppId>,
    /// Workspace index containing this window.
    pub workspace: usize,
    /// Whether the window is currently focused.
    pub focused: bool,
}

/// The Dock: pinned apps plus running-app state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Dock {
    /// Pinned apps, left-to-right.
    pub pinned: Vec<AppId>,
    /// Running apps mapped to their window counts.
    pub running: BTreeMap<AppId, usize>,
}

impl Dock {
    /// Pin an app; no-op if already pinned.
    pub fn pin(&mut self, app: AppId) {
        if !self.pinned.contains(&app) {
            self.pinned.push(app);
        }
    }

    /// Unpin an app; returns true if it was pinned.
    pub fn unpin(&mut self, app: &AppId) -> bool {
        let before = self.pinned.len();
        self.pinned.retain(|a| a != app);
        self.pinned.len() != before
    }

    /// Record windows per app from a window list.
    pub fn sync_running(&mut self, windows: &[WindowRecord]) {
        self.running.clear();
        for w in windows.iter().filter_map(|w| w.app.as_ref()) {
            *self.running.entry(w.clone()).or_default() += 1;
        }
    }
}

/// Workspaces: an ordered list with an active index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspaces {
    /// Names, in order.
    pub names: Vec<String>,
    /// Active workspace index.
    pub active: usize,
}

impl Workspaces {
    /// Create `count` workspaces named `1`..=`count`, first active.
    #[must_use]
    pub fn new(count: usize) -> Self {
        let count = count.max(1);
        Self {
            names: (1..=count).map(|i| i.to_string()).collect(),
            active: 0,
        }
    }

    /// Switch the active workspace; out-of-range is ignored.
    pub fn switch(&mut self, index: usize) {
        if index < self.names.len() {
            self.active = index;
        }
    }

    /// Move the window with `id` to another workspace; returns false when
    /// either index is invalid.
    pub fn move_window(&self, windows: &mut [WindowRecord], id: u64, to: usize) -> bool {
        if to >= self.names.len() {
            return false;
        }
        if let Some(w) = windows.iter_mut().find(|w| w.id == id) {
            w.workspace = to;
            return true;
        }
        false
    }
}

/// One launchable application parsed from a `.desktop` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherEntry {
    /// Entry id (desktop file basename).
    pub id: AppId,
    /// Display name (`Name=`).
    pub name: String,
    /// Executable line (`Exec=`), unexpanded.
    pub exec: String,
    /// Icon name or path (`Icon=`), if any.
    pub icon: Option<String>,
    /// Whether the app prefers a terminal (`Terminal=true`).
    pub terminal: bool,
    /// Skip entries marked `NoDisplay=true` / `Hidden=true`.
    pub hidden: bool,
}

/// Parse the `[Desktop Entry]` group of a `.desktop` file.
///
/// Only the keys the launcher needs are read; anything else is ignored.
/// Returns `None` when there is no usable `Name=` + `Exec=` pair.
#[must_use]
pub fn parse_desktop_entry(id: &str, content: &str) -> Option<LauncherEntry> {
    let mut in_group = false;
    let mut name = None;
    let mut exec = None;
    let mut icon = None;
    let mut terminal = false;
    let mut hidden = false;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_group = line == "[Desktop Entry]";
            continue;
        }
        if !in_group || line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        match k.trim() {
            "Name" if name.is_none() => name = Some(v.trim().to_owned()),
            "Exec" if exec.is_none() => exec = Some(v.trim().to_owned()),
            "Icon" if icon.is_none() => icon = Some(v.trim().to_owned()),
            "Terminal" => terminal = v.trim().eq_ignore_ascii_case("true"),
            "NoDisplay" | "Hidden" => {
                if v.trim().eq_ignore_ascii_case("true") {
                    hidden = true;
                }
            }
            "Type" if v.trim() != "Application" => return None,
            _ => {}
        }
    }
    Some(LauncherEntry {
        id: AppId::new(id),
        name: name?,
        exec: exec?,
        icon,
        terminal,
        hidden,
    })
}

/// Searchable index of launcher entries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LauncherIndex {
    /// All entries, including hidden ones (filtering happens at query time).
    pub entries: Vec<LauncherEntry>,
}

impl LauncherIndex {
    /// Case-insensitive substring search over id + name, skipping hidden
    /// entries. Empty query returns all visible entries.
    #[must_use]
    pub fn search(&self, query: &str) -> Vec<&LauncherEntry> {
        let q = query.to_lowercase();
        self.entries
            .iter()
            .filter(|e| !e.hidden)
            .filter(|e| {
                q.is_empty()
                    || e.name.to_lowercase().contains(&q)
                    || e.id.0.to_lowercase().contains(&q)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
