// SPDX-License-Identifier: MIT OR Apache-2.0
//
// The control centre.
//
// The toggles are inert: the real one drives NetworkManager, PipeWire and
// the Settings portal, none of which exist in a gallery. Showing them as
// working switches would be a lie, so each tile says what it would bind to.

// SwiftUI exists on Apple platforms and not on Linux. Guarding the whole
// file keeps the package buildable here, so `swift test` can still run the
// design-token and model tests. On iOS and macOS this compiles normally.
#if canImport(SwiftUI)
import SwiftUI
import TahoeDesign

struct ControlCentreView: View {
    let appearance: TahoeAppearance

    private struct Tile: Identifiable {
        let id: String
        let symbol: String
        let title: String
        let detail: String
        let presents: String
        var on: Bool
    }

    @State private var tiles: [Tile] = [
        Tile(id: "wifi", symbol: "wifi", title: "Wi-Fi", detail: "Connected",
             presents: "NetworkManager", on: true),
        Tile(id: "bt", symbol: "bluetooth", title: "Bluetooth", detail: "Off",
             presents: "BlueZ", on: false),
        Tile(id: "dnd", symbol: "moon.fill", title: "Focus", detail: "Off",
             presents: "xdg-desktop-portal", on: false),
        Tile(id: "audio", symbol: "speaker.wave.2", title: "Sound", detail: "72%",
             presents: "PipeWire", on: true),
    ]

    private let columns = [GridItem(.flexible(), spacing: 12), GridItem(.flexible(), spacing: 12)]

    var body: some View {
        LazyVGrid(columns: columns, spacing: Tahoe.spacing(.md, appearance)) {
            ForEach($tiles) { $tile in
                Button {
                    tile.on.toggle()
                } label: {
                    VStack(alignment: .leading, spacing: 6) {
                        HStack {
                            Image(systemName: tile.symbol)
                                .font(.system(size: 15))
                                .foregroundStyle(
                                    tile.on
                                        ? Tahoe.accentColor(appearance)
                                        : Tahoe.textMutedColor(appearance)
                                )
                            Spacer()
                            Text(tile.on ? "On" : "Off")
                                .font(.system(size: 10, weight: .medium))
                                .foregroundStyle(Tahoe.textMutedColor(appearance))
                        }
                        Text(tile.title)
                            .font(.system(size: 12, weight: .semibold))
                            .foregroundStyle(Tahoe.textColor(appearance))
                        Text(tile.detail)
                            .font(.system(size: 10))
                            .foregroundStyle(Tahoe.textMutedColor(appearance))
                        // Named so nobody mistakes this for a working switch.
                        Text(tile.presents)
                            .font(.system(size: 9, design: .monospaced))
                            .foregroundStyle(Tahoe.textMutedColor(appearance).opacity(0.7))
                    }
                    .padding(Tahoe.spacing(.md, appearance))
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(
                        RoundedRectangle(
                            cornerRadius: Tahoe.radius(.card, appearance),
                            style: .continuous
                        )
                        .fill(
                            tile.on
                                ? Tahoe.accentColor(appearance).opacity(0.15)
                                : Tahoe.textMutedColor(appearance).opacity(0.10)
                        )
                    )
                }
                .buttonStyle(.plain)
            }
        }
        .padding(Tahoe.spacing(.md, appearance))
        .background(Tahoe.windowBackgroundColor(appearance))
    }
}
#endif
