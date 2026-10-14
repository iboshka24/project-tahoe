// SPDX-License-Identifier: MIT OR Apache-2.0
//
// The application entry point.
//
// xtool packages a SwiftPM library, so the `@main` attribute lives here
// rather than on an `@NSApplicationMain`/`@UIApplicationMain` type.

import SwiftUI
import TahoeDesign

@main
struct TahoeGalleryApp: App {
    /// Which appearance the gallery is showing.
    ///
    /// Defaults to the same value the Rust side does, so a screenshot from
    /// this app and a `tahoe-preview` render agree out of the box.
    @State private var appearance: TahoeAppearance = Tahoe.defaultAppearance

    var body: some Scene {
        WindowGroup {
            ContentView(appearance: $appearance)
                .preferredColorScheme(appearance == .dark ? .dark : .light)
        }
    }
}
