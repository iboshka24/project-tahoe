// swift-tools-version: 6.0
// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Tahoe Gallery: a SwiftUI application that renders the Tahoe design system
// on Apple platforms, built on Linux with xtool.
//
// xtool turns a SwiftPM *library* into an iOS .app, which is why the product
// below is a `.library` and `TahoeGalleryApp` carries `@main` itself.
//
// Build:  xtool dev build        (writes ./xtool/TahoeGallery.app)
// Test:   swift test            (tokens only; no UI framework needed)
//
// See README.md in this directory for the full story, including what xtool
// needs that this repository deliberately does not ship.

import PackageDescription

let package = Package(
    name: "TahoeGallery",
    platforms: [
        // xtool reads the deployment target from here and writes it into
        // `MinimumOSVersion` in the generated Info.plist.
        .iOS(.v17),
        .macOS(.v14),
    ],
    products: [
        .library(name: "TahoeGallery", targets: ["TahoeGallery"]),
    ],
    targets: [
        // Design tokens, generated from the Rust design system. No imports,
        // so this target builds and tests without a UI framework.
        .target(name: "TahoeDesign"),

        // The app itself. SwiftUI is provided by the iOS SDK that xtool
        // extracts from the Xcode archive.
        .target(
            name: "TahoeGallery",
            dependencies: ["TahoeDesign"]
        ),

        // Token assertions. These run with plain `swift test` on Linux, so a
        // palette regression fails a headless job rather than an iPhone
        // build that nobody runs in CI.
        .testTarget(
            name: "TahoeDesignTests",
            dependencies: ["TahoeDesign"]
        ),
    ]
)
