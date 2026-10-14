// SPDX-License-Identifier: MIT OR Apache-2.0
//
// State for the gallery.
//
// This mirrors the Rust shell model in `crates/tahoe-shell/src/lib.rs` —
// the same Dock, the same workspaces, the same launcher index — so the two
// front ends can be compared item by item. It is deliberately a plain value
// type with no SwiftUI dependency, for the same reason the Rust model has
// none: it is the part worth testing.

import Foundation

/// An application the gallery can show.
struct GalleryApp: Identifiable, Hashable {
    let id: String
    let name: String
    /// SF Symbol name. Stands in for an icon file; the real shell reads the
    /// `.desktop` entry's `Icon=` and resolves it through the icon theme.
    let symbol: String
    /// Whether it is running, which the dock shows as a dot.
    var isRunning: Bool = false
}

/// The dock: pinned items plus running state, matching `Dock` in Rust.
struct Dock: Equatable {
    var pinned: [GalleryApp] = []
    var running: Set<String> = []

    /// The dock in display order.
    var items: [GalleryApp] {
        pinned.map { app in
            var copy = app
            copy.isRunning = running.contains(app.id)
            return copy
        }
    }
}

/// A window the gallery draws inside its own canvas.
///
/// The real compositor owns windows; this is a rendering of one, so the
/// chrome can be demonstrated without a window server.
struct GalleryWindow: Identifiable, Equatable {
    let id: Int
    var title: String
    var app: String
    var isFocused: Bool
}

/// Everything the gallery displays.
struct GalleryModel {
    var dock = Dock(pinned: [
        GalleryApp(id: "finder", name: "Finder", symbol: "folder"),
        GalleryApp(id: "launcher", name: "Launcher", symbol: "square.grid.2x2"),
        GalleryApp(id: "files", name: "Files", symbol: "doc.text"),
        GalleryApp(id: "terminal", name: "Terminal", symbol: "terminal"),
        GalleryApp(id: "settings", name: "Settings", symbol: "gearshape"),
        GalleryApp(id: "wine", name: "Windows", symbol: "pc"),
        GalleryApp(id: "darling", name: "macOS", symbol: "apple.logo"),
    ])

    var windows: [GalleryWindow] = [
        GalleryWindow(id: 1, title: "Tahoe Gallery", app: "finder", isFocused: true),
        GalleryWindow(id: 2, title: "Universal Execution", app: "files", isFocused: false),
        GalleryWindow(id: 3, title: "Compatibility Matrix", app: "doc.text", isFocused: false),
    ]

    /// The launcher's index. In the real shell this comes from parsing
    /// `.desktop` files in the XDG data directories.
    var launcherEntries: [GalleryApp] = [
        GalleryApp(id: "org.tahoe.Gallery", name: "Tahoe Gallery", symbol: "photo.on.rectangle"),
        GalleryApp(id: "org.tahoe.Files", name: "Tahoe Files", symbol: "folder"),
        GalleryApp(id: "org.tahoe.Launcher", name: "Launcher", symbol: "magnifyingglass"),
        GalleryApp(id: "org.tahoe.Settings", name: "System Settings", symbol: "gearshape.2"),
        GalleryApp(id: "org.tahoe.ControlCentre", name: "Control Centre", symbol: "slider.horizontal.3"),
    ]

    var workspaces: [String] = ["1", "2", "3", "4"]
    var activeWorkspace: Int = 0

    /// The focused window, if any. Mirrors `WindowStack::focused` in Rust.
    var focused: GalleryWindow? {
        windows.first { $0.isFocused }
    }

    /// Case-insensitive substring search over the launcher, skipping hidden
    /// entries. The same contract as `LauncherIndex::search`.
    func search(_ query: String) -> [GalleryApp] {
        let needle = query.lowercased()
        guard !needle.isEmpty else { return launcherEntries }
        return launcherEntries.filter {
            $0.name.lowercased().contains(needle) || $0.id.lowercased().contains(needle)
        }
    }
}
