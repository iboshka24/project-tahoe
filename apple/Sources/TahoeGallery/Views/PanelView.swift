// SPDX-License-Identifier: MIT OR Apache-2.0
//
// The top panel, the strip the compositor reserves.
//
// `Tahoe.panelHeight` is exactly `tahoe_compositor::PANEL_HEIGHT`, which is
// exactly `DesignSystem::metrics.panel_height`. A window that assumed any
// other value would either float with a gap above it or slide under the
// panel, so the number is generated rather than repeated.

// SwiftUI exists on Apple platforms and not on Linux. Guarding the whole
// file keeps the package buildable here, so `swift test` can still run the
// design-token and model tests. On iOS and macOS this compiles normally.
#if canImport(SwiftUI)
import SwiftUI
import TahoeDesign

struct PanelView: View {
    let appearance: TahoeAppearance
    let activeWorkspace: Int
    let workspaces: [String]
    @Binding var appearanceBinding: TahoeAppearance

    private var panelHeight: CGFloat { Tahoe.panelHeight(appearance) }

    var body: some View {
        HStack(spacing: Tahoe.spacing(.sm, appearance)) {
            // The menu side.
            Image(systemName: "apple.logo")
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(Tahoe.textColor(appearance))

            Text("Gallery")
                .font(.system(size: 13, weight: .semibold))
                .foregroundStyle(Tahoe.textColor(appearance))

            ForEach(["File", "Edit", "View"], id: \.self) { item in
                Text(item)
                    .font(.system(size: 12))
                    .foregroundStyle(Tahoe.textMutedColor(appearance))
            }

            Spacer()

            // Workspace pills. The real shell switches `Space` groups here.
            HStack(spacing: 4) {
                ForEach(Array(workspaces.enumerated()), id: \.offset) { index, name in
                    Text(name)
                        .font(.system(size: 11, weight: .medium, design: .rounded))
                        .foregroundStyle(
                            index == activeWorkspace
                                ? Tahoe.textColor(appearance)
                                : Tahoe.textMutedColor(appearance)
                        )
                        .frame(width: 18, height: 16)
                        .background(
                            RoundedRectangle(cornerRadius: 4, style: .continuous)
                                .fill(
                                    index == activeWorkspace
                                        ? Tahoe.accentColor(appearance).opacity(0.85)
                                        : Color.clear
                                )
                        )
                }
            }

            // The appearance switch. In the real desktop this comes from the
            // Settings portal, which is why `Tahoe.defaultAppearance` is a
            // documented constant rather than a detected value.
            Button {
                appearanceBinding = appearance == .dark ? .light : .dark
            } label: {
                Image(systemName: appearance == .dark ? "moon.fill" : "sun.max.fill")
                    .font(.system(size: 12))
                    .foregroundStyle(Tahoe.textColor(appearance))
            }
            .buttonStyle(.plain)
            .help("Toggle appearance")

            Image(systemName: "wifi")
                .font(.system(size: 12))
                .foregroundStyle(Tahoe.textColor(appearance))
            Image(systemName: "battery.75")
                .font(.system(size: 12))
                .foregroundStyle(Tahoe.textColor(appearance))
            Text("Tahoe")
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(Tahoe.textColor(appearance))
        }
        .padding(.horizontal, Tahoe.spacing(.md, appearance))
        .frame(height: panelHeight)
        .frame(maxWidth: .infinity)
        .background(Tahoe.panelColor(appearance))
        .overlay(alignment: .bottom) {
            Rectangle()
                .fill(Tahoe.separatorColor(appearance))
                .frame(height: 1)
        }
    }
}
#endif
