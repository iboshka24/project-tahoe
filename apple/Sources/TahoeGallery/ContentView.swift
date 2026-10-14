// SPDX-License-Identifier: MIT OR Apache-2.0
//
// The gallery: the whole design system on one screen.
//
// Layered bottom to top exactly as the compositor composites it —
// wallpaper, then chrome, then windows — so a screenshot here and a
// `tahoe-preview` render can be compared directly.

// SwiftUI exists on Apple platforms and not on Linux. Guarding the whole
// file keeps the package buildable here, so `swift test` can still run the
// design-token and model tests. On iOS and macOS this compiles normally.
#if canImport(SwiftUI)
import SwiftUI
import TahoeDesign

struct ContentView: View {
    @Binding var appearance: TahoeAppearance
    @State private var model = GalleryModel()
    @State private var showLauncher = false

    var body: some View {
        ZStack {
            // 1. Wallpaper. The same gradient the compositor uploads as a
            //    texture, generated from the same stops.
            Tahoe.wallpaperGradient(appearance)
                .ignoresSafeArea()

            // 2. Windows, in the model's order, inset below the panel the
            //    way the compositor's work area is.
            GeometryReader { geometry in
                let area = workArea(in: geometry.size)
                ForEach(Array(model.windows.enumerated()), id: \.element.id) { index, window in
                    WindowCardView(window: window, appearance: appearance)
                        .frame(width: area.width * 0.52, height: area.height * 0.62)
                        .offset(
                            x: area.minX + CGFloat(index) * 34,
                            y: area.minY + CGFloat(index) * 30
                        )
                        .onTapGesture { focus(window.id) }
                }
            }

            // 3. Chrome.
            VStack(spacing: 0) {
                PanelView(
                    appearance: appearance,
                    activeWorkspace: model.activeWorkspace,
                    workspaces: model.workspaces,
                    appearanceBinding: $appearance
                )

                if showLauncher {
                    LauncherView(entries: model.launcherEntries, appearance: appearance)
                        .frame(maxWidth: 420)
                        .frame(maxHeight: 340)
                        .clipShape(
                            RoundedRectangle(
                                cornerRadius: Tahoe.radius(.window, appearance),
                                style: .continuous
                            )
                        )
                        .shadow(color: .black.opacity(0.3), radius: 24, y: 8)
                        .padding(.top, Tahoe.spacing(.xl, appearance))
                        .transition(.scale(scale: 0.96).combined(with: .opacity))
                }

                Spacer()

                DockView(dock: model.dock, appearance: appearance) { app in
                    toggleRunning(app)
                    if app.id == "launcher" {
                        withAnimation(.easeOut(duration: 0.15)) { showLauncher.toggle() }
                    }
                }
                .padding(.bottom, Tahoe.spacing(.lg, appearance))
            }
        }
        // The panel is opaque-strip-only; everything else is the wallpaper.
        .frame(minWidth: 900, minHeight: 600)
    }

    /// The work area: everything below the reserved panel strip.
    ///
    /// Mirrors `tahoe_compositor::work_area`, which is why the panel height
    /// is a generated token rather than a local constant.
    private func workArea(in size: CGSize) -> CGRect {
        let panel = Tahoe.panelHeight(appearance)
        return CGRect(
            x: Tahoe.spacing(.xl, appearance),
            y: panel + Tahoe.spacing(.xl, appearance),
            width: size.width - Tahoe.spacing(.xl, appearance) * 2,
            height: size.height - panel - Tahoe.spacing(.xl, appearance) * 2
                - Tahoe.dockHeight(appearance)
        )
    }

    private func focus(_ id: Int) {
        for index in model.windows.indices {
            model.windows[index].isFocused = model.windows[index].id == id
        }
    }

    private func toggleRunning(_ app: GalleryApp) {
        if model.dock.running.contains(app.id) {
            model.dock.running.remove(app.id)
        } else {
            model.dock.running.insert(app.id)
        }
    }
}
#endif
