// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Model tests, runnable on Linux.
//
// `GalleryModel` deliberately has no SwiftUI dependency, so this target
// builds and runs under a plain `swift test` with no SDK. That matters: the
// launcher's search contract is the same one `LauncherIndex::search`
// implements in Rust, and asserting both sides against the same rules is a
// cross-language behavioural check rather than two independent guesses.

import XCTest
@testable import TahoeGallery

final class ModelTests: XCTestCase {

    func testSearchIsCaseInsensitiveOverNameAndIdentifier() {
        let model = GalleryModel()
        XCTAssertEqual(model.search("GALLERY").count, 1)
        XCTAssertEqual(model.search("gallery").count, 1)
        // Matching on the identifier, not only the display name.
        XCTAssertEqual(model.search("org.tahoe.Launcher").count, 1)
        XCTAssertEqual(model.search("launcher").count, 1)
    }

    func testAnEmptyQueryReturnsEverything() {
        let model = GalleryModel()
        XCTAssertFalse(model.search("").isEmpty)
        XCTAssertEqual(model.search("").count, model.launcherEntries.count)
    }

    func testAMissReturnsNothingRatherThanEverything() {
        // The failure mode worth pinning: a search that falls back to the
        // full list when it finds nothing looks like it works and is useless.
        let model = GalleryModel()
        XCTAssertTrue(model.search("no-such-application-anywhere").isEmpty)
    }

    func testTheDockStartsWithSevenPinnedItemsAndNoneRunning() {
        let model = GalleryModel()
        XCTAssertEqual(model.dock.items.count, 7)
        XCTAssertTrue(model.dock.running.isEmpty)
        XCTAssertTrue(model.dock.items.allSatisfy { !$0.isRunning })
    }

    func testRunningStateFollowsTheDock() {
        var model = GalleryModel()
        model.dock.running.insert("finder")
        let finder = model.dock.items.first { $0.id == "finder" }
        XCTAssertTrue(finder?.isRunning == true)
        let other = model.dock.items.first { $0.id == "terminal" }
        XCTAssertFalse(other?.isRunning == true)
    }

    func testDockOrderIsThePinnedOrder() {
        // Reordering the dock must not silently move items; the Rust Dock
        // has the same guarantee.
        let model = GalleryModel()
        XCTAssertEqual(
            model.dock.items.map(\.id),
            ["finder", "launcher", "files", "terminal", "settings", "wine", "darling"]
        )
    }

    func testExactlyOneWindowIsFocused() {
        let model = GalleryModel()
        XCTAssertEqual(model.windows.filter(\.isFocused).count, 1)
        XCTAssertEqual(model.focused?.id, 1)
    }

    func testWorkspacesStartOnTheFirst() {
        let model = GalleryModel()
        XCTAssertEqual(model.activeWorkspace, 0)
        XCTAssertEqual(model.workspaces.count, 4)
    }
}
