// SPDX-License-Identifier: MIT OR Apache-2.0
//
// The launcher.
//
// The search contract matches `LauncherIndex::search` in Rust: a
// case-insensitive substring over id and name, empty query returns
// everything, hidden entries never surface.

// SwiftUI exists on Apple platforms and not on Linux. Guarding the whole
// file keeps the package buildable here, so `swift test` can still run the
// design-token and model tests. On iOS and macOS this compiles normally.
#if canImport(SwiftUI)
import SwiftUI
import TahoeDesign

struct LauncherView: View {
    let entries: [GalleryApp]
    let appearance: TahoeAppearance
    @State private var query = ""

    private var results: [GalleryApp] {
        let needle = query.lowercased()
        guard !needle.isEmpty else { return entries }
        return entries.filter {
            $0.name.lowercased().contains(needle) || $0.id.lowercased().contains(needle)
        }
    }

    var body: some View {
        VStack(spacing: Tahoe.spacing(.md, appearance)) {
            // Search field.
            HStack(spacing: 8) {
                Image(systemName: "magnifyingglass")
                    .foregroundStyle(Tahoe.textMutedColor(appearance))
                TextField("Search", text: $query)
                    .textFieldStyle(.plain)
                    .font(.system(size: 14))
                    .foregroundStyle(Tahoe.textColor(appearance))
            }
            .padding(.horizontal, Tahoe.spacing(.md, appearance))
            .frame(height: 32)
            .background(
                RoundedRectangle(
                    cornerRadius: Tahoe.radius(.control, appearance),
                    style: .continuous
                )
                .fill(Tahoe.textMutedColor(appearance).opacity(0.15))
            )

            if results.isEmpty {
                // A real empty state, not a blank panel.
                VStack(spacing: 6) {
                    Image(systemName: "questionmark.circle")
                        .font(.system(size: 24))
                        .foregroundStyle(Tahoe.textMutedColor(appearance))
                    Text("No applications match \(query)")
                        .font(.system(size: 12))
                        .foregroundStyle(Tahoe.textMutedColor(appearance))
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ScrollView {
                    LazyVStack(spacing: 2) {
                        ForEach(results) { app in
                            HStack(spacing: Tahoe.spacing(.sm, appearance)) {
                                Image(systemName: app.symbol)
                                    .font(.system(size: 14))
                                    .frame(width: 22)
                                    .foregroundStyle(Tahoe.textColor(appearance))
                                VStack(alignment: .leading, spacing: 1) {
                                    Text(app.name)
                                        .font(.system(size: 13))
                                        .foregroundStyle(Tahoe.textColor(appearance))
                                    Text(app.id)
                                        .font(.system(size: 10))
                                        .foregroundStyle(Tahoe.textMutedColor(appearance))
                                }
                                Spacer()
                            }
                            .padding(.horizontal, Tahoe.spacing(.sm, appearance))
                            .frame(height: Tahoe.radius(.card, appearance) + 22)
                            .background(
                                RoundedRectangle(
                                    cornerRadius: Tahoe.radius(.card, appearance),
                                    style: .continuous
                                )
                                .fill(Tahoe.textMutedColor(appearance).opacity(0.08))
                            )
                        }
                    }
                }
            }
        }
        .padding(Tahoe.spacing(.md, appearance))
        .background(Tahoe.windowBackgroundColor(appearance))
    }
}
#endif
