// SPDX-License-Identifier: MIT OR Apache-2.0
//
// The dock.
//
// Mirrors `Dock` in `crates/tahoe-shell/src/lib.rs`: pinned items in order,
// running state as a dot. The geometry comes from the generated metrics, so
// the SwiftUI dock and a `tahoe-preview` render are the same size.

import SwiftUI
import TahoeDesign

struct DockView: View {
    let dock: Dock
    let appearance: TahoeAppearance
    let onActivate: (GalleryApp) -> Void

    private var corner: CGFloat { Tahoe.radius(.dock, appearance) }
    private var iconSize: CGFloat { Tahoe.dockIconSize(appearance) }

    var body: some View {
        HStack(spacing: Tahoe.spacing(.md, appearance)) {
            ForEach(dock.items) { app in
                Button {
                    onActivate(app)
                } label: {
                    VStack(spacing: 3) {
                        Image(systemName: app.symbol)
                            .font(.system(size: iconSize * 0.5, weight: .regular))
                            .foregroundStyle(Tahoe.textColor(appearance))
                            .frame(width: iconSize, height: iconSize)
                            .background(
                                RoundedRectangle(
                                    cornerRadius: Tahoe.radius(.card, appearance),
                                    style: .continuous
                                )
                                .fill(Tahoe.accentColor(appearance).opacity(app.isRunning ? 0.22 : 0.10))
                            )
                        // The running indicator. A dot, not a change of size,
                        // so the dock does not reflow when an app starts.
                        Circle()
                            .fill(app.isRunning ? Tahoe.accentColor(appearance) : Color.clear)
                            .frame(width: 3, height: 3)
                    }
                }
                .buttonStyle(.plain)
                .help(app.name)
            }
        }
        .padding(.horizontal, Tahoe.spacing(.md, appearance))
        .padding(.vertical, Tahoe.spacing(.sm, appearance))
        .background(
            RoundedRectangle(cornerRadius: corner, style: .continuous)
                .fill(Tahoe.dockColor(appearance))
        )
        .overlay(
            RoundedRectangle(cornerRadius: corner, style: .continuous)
                .stroke(Tahoe.separatorColor(appearance), lineWidth: 1)
        )
    }
}
