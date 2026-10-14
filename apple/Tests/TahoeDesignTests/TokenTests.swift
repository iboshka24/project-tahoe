// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Token assertions that run headlessly.
//
// `TahoeDesign.swift` deliberately imports nothing, so these run under a
// plain `swift test` on Linux with no iOS SDK and no SwiftUI. That matters:
// a palette regression should fail a cheap job, not an iPhone build that
// nobody runs in CI.
//
// These are the *Swift half* of the check. The Rust half is
// `the_committed_swift_file_matches_the_rust_design_system`, which
// regenerates the file and compares bytes. Between them, neither language
// can drift without a failure.

import XCTest
@testable import TahoeDesign

final class TokenTests: XCTestCase {

    func testBothAppearancesExist() {
        XCTAssertTrue(TahoeDesign_Dark.isDark)
        XCTAssertFalse(TahoeDesign_Light.isDark)
        XCTAssertEqual(TahoeDesign_Dark.appearanceName, "Tahoe Dark")
        XCTAssertEqual(TahoeDesign_Light.appearanceName, "Tahoe Light")
    }

    func testPanelHeightIsTheReservedStrip() {
        // The compositor reserves exactly this many points. A window that
        // assumed anything else would sit under the panel.
        XCTAssertEqual(TahoeDesign_Dark.panelHeight, 28)
        XCTAssertEqual(TahoeDesign_Light.panelHeight, 28)
    }

    func testLayoutIsSharedBetweenAppearances() {
        // Layout shifting between light and dark would be a bug users notice
        // as the dock jumping when they switch.
        for (dark, light) in [
            (TahoeDesign_Dark.spacingXs, TahoeDesign_Light.spacingXs),
            (TahoeDesign_Dark.spacingXl, TahoeDesign_Light.spacingXl),
            (TahoeDesign_Dark.radiusDock, TahoeDesign_Light.radiusDock),
            (TahoeDesign_Dark.radiusWindow, TahoeDesign_Light.radiusWindow),
            (TahoeDesign_Dark.dockHeight, TahoeDesign_Light.dockHeight),
            (TahoeDesign_Dark.dockIcon, TahoeDesign_Light.dockIcon),
            (TahoeDesign_Dark.sidebarWidth, TahoeDesign_Light.sidebarWidth),
        ] {
            XCTAssertEqual(dark, light, "layout must not differ between appearances")
        }
    }

    func testChromeColoursDifferBetweenAppearances() {
        XCTAssertNotEqual(TahoeDesign_Dark.text, TahoeDesign_Light.text)
        XCTAssertNotEqual(TahoeDesign_Dark.panel, TahoeDesign_Light.panel)
        XCTAssertNotEqual(TahoeDesign_Dark.windowBackground, TahoeDesign_Light.windowBackground)
    }

    func testAccentIsSharedAndOpaque() {
        XCTAssertEqual(TahoeDesign_Dark.accent, TahoeDesign_Light.accent)
        XCTAssertEqual(TahoeDesign_Dark.accent.alpha, 1.0, accuracy: 1e-6)
    }

    func testChromeIsTranslucentAndWallpaperIsNot() {
        // The panel and dock are materials over the wallpaper; the wallpaper
        // itself must be opaque or the desktop shows through to black.
        XCTAssertLessThan(TahoeDesign_Dark.panel.alpha, 1.0)
        XCTAssertLessThan(TahoeDesign_Dark.dock.alpha, 1.0)
        XCTAssertEqual(TahoeDesign_Dark.wallpaperColor(at: 0.5).alpha, 1.0, accuracy: 1e-6)
        XCTAssertEqual(TahoeDesign_Light.wallpaperColor(at: 0.5).alpha, 1.0, accuracy: 1e-6)
    }

    func testWallpaperIsARealMonotonicGradient() {
        for isDark in [true, false] {
            // Explicit closures rather than unapplied method references:
            // a bare `Type.method(at:)` inside a ternary defeats the type
            // checker ("failed to produce diagnostic"), which is a Swift
            // limitation and not a problem with the code being tested.
            let sample: (Double) -> RGBA = isDark
                ? { t in TahoeDesign_Dark.wallpaperColor(at: t) }
                : { t in TahoeDesign_Light.wallpaperColor(at: t) }
            let top = sample(0)
            let bottom = sample(1)
            XCTAssertNotEqual(top, bottom, "the wallpaper must not be a flat colour")

            var previous = -1.0
            for step in 0...20 {
                let colour = sample(Double(step) / 20)
                let luminance = colour.red + colour.green + colour.blue
                XCTAssertGreaterThanOrEqual(
                    luminance, previous - 1e-9,
                    "the gradient darkens at step \(step)"
                )
                previous = luminance
            }
        }
    }

    func testWallpaperSamplerClampsOutOfRangeInput() {
        XCTAssertEqual(TahoeDesign_Dark.wallpaperColor(at: -5), TahoeDesign_Dark.wallpaperColor(at: 0))
        XCTAssertEqual(TahoeDesign_Dark.wallpaperColor(at: 5), TahoeDesign_Dark.wallpaperColor(at: 1))
    }

    func testWallpaperStopsAreOrderedAndBounded() {
        for stops in [TahoeDesign_Dark.wallpaper, TahoeDesign_Light.wallpaper] {
            XCTAssertFalse(stops.isEmpty)
            var previous = -1.0
            for stop in stops {
                XCTAssertGreaterThanOrEqual(stop.position, previous, "stops must be ordered")
                XCTAssertGreaterThanOrEqual(stop.position, 0)
                XCTAssertLessThanOrEqual(stop.position, 1)
                previous = stop.position
            }
            XCTAssertEqual(stops.first?.position, 0)
            XCTAssertEqual(stops.last?.position, 1)
        }
    }

    func testSpacingScaleIsIncreasing() {
        XCTAssertLessThan(TahoeDesign_Dark.spacingXs, TahoeDesign_Dark.spacingSm)
        XCTAssertLessThan(TahoeDesign_Dark.spacingSm, TahoeDesign_Dark.spacingMd)
        XCTAssertLessThan(TahoeDesign_Dark.spacingMd, TahoeDesign_Dark.spacingLg)
        XCTAssertLessThan(TahoeDesign_Dark.spacingLg, TahoeDesign_Dark.spacingXl)
    }

    func testRadiiAreOrdered() {
        XCTAssertLessThan(TahoeDesign_Dark.radiusControl, TahoeDesign_Dark.radiusCard)
        XCTAssertLessThan(TahoeDesign_Dark.radiusCard, TahoeDesign_Dark.radiusWindow)
        XCTAssertLessThan(TahoeDesign_Dark.radiusWindow, TahoeDesign_Dark.radiusDock)
    }

    func testAppearanceResolverAgreesWithTheNamespaces() {
        for appearance in TahoeAppearance.allCases {
            for step in [0.0, 0.25, 0.5, 0.75, 1.0] {
                let viaResolver = Tahoe.wallpaper(at: step, appearance: appearance)
                let direct = appearance == .dark
                    ? TahoeDesign_Dark.wallpaperColor(at: step)
                    : TahoeDesign_Light.wallpaperColor(at: step)
                XCTAssertEqual(viaResolver, direct, "\(appearance) at \(step)")
            }
        }
    }

    func testDefaultAppearanceIsDocumented() {
        // Dark, because the Settings portal that would report the user's
        // preference is not implemented on the Tahoe side. If this changes,
        // the Rust default must change with it.
        XCTAssertEqual(Tahoe.defaultAppearance, .dark)
        XCTAssertEqual(TahoeAppearance.allCases.count, 2)
    }
}
