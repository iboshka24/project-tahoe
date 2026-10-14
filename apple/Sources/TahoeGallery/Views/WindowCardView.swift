// SPDX-License-Identifier: MIT OR Apache-2.0
//
// A window, drawn by the gallery rather than by a compositor.
//
// The real Tahoe compositor draws client surfaces; this renders the chrome
// it would put around them, so the design can be reviewed without a Wayland
// session. The corner radius and border colour are the same tokens the
// compositor uses.

// SwiftUI exists on Apple platforms and not on Linux. Guarding the whole
// file keeps the package buildable here, so `swift test` can still run the
// design-token and model tests. On iOS and macOS this compiles normally.
#if canImport(SwiftUI)
import SwiftUI
import TahoeDesign

struct WindowCardView: View {
    let window: GalleryWindow
    let appearance: TahoeAppearance

    private var corner: CGFloat { Tahoe.radius(.window, appearance) }

    var body: some View {
        VStack(spacing: 0) {
            // Title bar. macOS clients draw their own; this is what the
            // compositor would draw for a client that asked for server-side
            // decoration, which Tahoe does not implement yet.
            HStack(spacing: 6) {
                Circle()
                    .fill(Color(red: 1.0, green: 0.37, blue: 0.34))
                    .frame(width: 11, height: 11)
                Circle()
                    .fill(Color(red: 1.0, green: 0.74, blue: 0.20))
                    .frame(width: 11, height: 11)
                Circle()
                    .fill(Color(red: 0.24, green: 0.79, blue: 0.30))
                    .frame(width: 11, height: 11)

                Spacer()

                Text(window.title)
                    .font(.system(size: 12, weight: window.isFocused ? .semibold : .regular))
                    .foregroundStyle(
                        window.isFocused
                            ? Tahoe.textColor(appearance)
                            : Tahoe.textMutedColor(appearance)
                    )

                Spacer()

                // Balances the traffic lights so the title stays centred.
                Color.clear.frame(width: 45, height: 1)
            }
            .padding(.horizontal, Tahoe.spacing(.md, appearance))
            .frame(height: 30)
            .background(Tahoe.windowBackgroundColor(appearance))

            Rectangle()
                .fill(Tahoe.separatorColor(appearance))
                .frame(height: 1)

            // Body. In a real window this is the client's surface.
            VStack(alignment: .leading, spacing: Tahoe.spacing(.sm, appearance)) {
                Text(window.app)
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(Tahoe.textColor(appearance))
                ForEach(0..<3, id: \.self) { row in
                    RoundedRectangle(cornerRadius: 3, style: .continuous)
                        .fill(Tahoe.textMutedColor(appearance).opacity(0.25))
                        .frame(height: 8)
                        .frame(maxWidth: row == 2 ? 140 : .infinity, alignment: .leading)
                }
                Spacer(minLength: 0)
            }
            .padding(Tahoe.spacing(.md, appearance))
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Tahoe.windowBackgroundColor(appearance))
        }
        .clipShape(RoundedRectangle(cornerRadius: corner, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: corner, style: .continuous)
                .stroke(
                    window.isFocused
                        ? Tahoe.accentColor(appearance)
                        : Tahoe.separatorColor(appearance),
                    lineWidth: window.isFocused ? 1.5 : 1
                )
        )
        .shadow(color: .black.opacity(0.25), radius: 12, y: 4)
    }
}
#endif
