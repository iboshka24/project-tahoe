# Building Apple applications on Linux

> How Tahoe builds, and inspects, iOS and macOS applications without a Mac —
> and exactly where the honest limits are.

---

## 1. The capability, stated precisely

| | Status |
|---|---|
| Write a real SwiftUI application | done — `apple/` |
| Build it for iOS **on Linux** | possible, via xtool; **not attempted here** |
| Run it | **not possible on Linux.** A real iPhone, or macOS. |
| Run it under Darling | **no.** It links UIKit, which Darling does not implement. |
| Inspect the built `.app` | done — `tahoe-compat apple inspect` |
| Keep the SwiftUI palette identical to the compositor's | done, structurally |

The last row is the interesting one for this project, and it is why the
SwiftUI application is not a toy: it is the second consumer of the same
design system, and the two are kept in step by code generation rather than
by discipline.

## 2. One design system, two languages

The Rust [`DesignSystem`](../crates/tahoe-shell/src/design.rs) is the single
source of truth. `apple/Sources/TahoeDesign/TahoeDesign.swift` is
**generated**:

```sh
cargo run -p tahoe-shell --bin tahoe-design-export -- \
    apple/Sources/TahoeDesign/TahoeDesign.swift

# CI: fail if the committed file is stale
cargo run -p tahoe-shell --bin tahoe-design-export -- --check \
    apple/Sources/TahoeDesign/TahoeDesign.swift
```

Three independent checks keep the two sides honest:

1. `tahoe-shell` regenerates the file in a test and compares bytes, so
   editing the Swift by hand fails the Rust build.
2. The same test **parses the emitted Swift back** — `RGBA(...)`,
   `spacingXs`, `panelHeight` — and compares every value to the Rust
   system, so a change to the emitter cannot silently corrupt the tokens.
3. `apple/Tests/TahoeDesignTests` asserts the numbers from the Swift side.
   The generated file imports nothing, so those tests run under a plain
   `swift test` on Linux with no SDK and no SwiftUI — a palette regression
   fails a cheap job rather than an iPhone build nobody runs.

The panel height is the value that would hurt most if it drifted:
`tahoe_compositor::PANEL_HEIGHT` is 28, `DesignSystem::metrics.panel_height`
is 28, and `TahoeDesign.panelHeight` is 28. A window that assumed anything
else would either float with a gap or slide under the panel, and the tests
pin it in all three places.

## 3. xtool

[xtool](https://github.com/xtool-org/xtool) (MIT, ~10k stars, active) turns a
SwiftPM **library** package into an iOS `.app` on Linux, driving a real Swift
toolchain with an iOS target triple and the real Apple SDK. It is the only
tool that does this; nothing else builds SwiftUI for iOS on Linux.

### What it needs, and what Tahoe will not fetch

| Requirement | Notes |
|---|---|
| Swift **6.4** toolchain | from <https://swift.org/install/linux>. A tarball, no root needed. |
| **`Xcode.xip`** | ~7 GB, from <https://developer.apple.com/download/all/?q=Xcode>, **signed in with your Apple ID** |
| xtool | an AppImage, no root needed |

**Apple's SDK is not ours to ship or download.** xtool requires *you* to
fetch `Xcode.xip` under your own Apple ID and accept Apple's license; it
extracts the iOS SDK locally and installs a Swift SDK named `darwin`. Whether
the extracted SDK may be redistributed is not addressed anywhere in xtool's
documentation, so **do not assume it may be**. Tahoe never downloads an SDK
and never copies one — `tahoe-compat` only inspects a prefix you already
installed.

### The build

```sh
xtool sdk install /path/to/Xcode.xip     # once
swift sdk list                            # should show `darwin`

cd apple
xtool dev build                           # writes ./xtool/TahoeGallery.app
```

**No Apple developer account is needed** for `dev build`. Signing happens
only with `--sign`, which needs an Apple ID and a real device — so
`tahoe-compat` deliberately never passes it.

Running needs a physical device:

```sh
xtool auth
xtool dev run
```

There is no simulator on Linux.

## 4. Closing the loop: real artifacts feed the parsers

Before this, every `.app` that `tahoe-compat::macos` parsed was synthetic.
xtool changes that: `xtool dev build` produces a genuine bundle, and

```sh
cargo run -p tahoe-compat -- apple inspect apple/xtool/TahoeGallery.app
```

runs the same code path a user would.

### Three real bugs this immediately exposed

The parsers were written against fixtures that were *plausible* rather than
*realistic*, and three things were wrong:

1. **iOS bundles are flat.** macOS is
   `Foo.app/Contents/{Info.plist,MacOS/Foo}`; iOS is `Foo.app/{Info.plist,Foo}`
   with no `Contents/` at all. `AppBundle` only knew the macOS layout, so it
   would have rejected every app xtool produced. Now
   `BundleLayout::{MacOs, Ios}`.
2. **iOS writes `MinimumOSVersion`, macOS writes `LSMinimumSystemVersion`.**
   Reading only the second meant every iOS app reported no minimum at all.
3. **`<dict/>` was not parsed.** A self-closing empty dict is everywhere in
   real plists — every `UILaunchScreen` is one — and the XML reader only
   recognised `<dict>`.

None of these would have been found by unit tests written from the same
understanding that produced the bug.

A fourth bug was in the other direction: the XML reader's `peek_tag` stopped
at the first `/`, so `</array>` looked like an empty start tag and array
parsing failed outright.

## 5. The application

`apple/Sources/TahoeGallery/` is a SwiftUI gallery that renders the whole
design system — the panel, the dock, a window with the real corner radius,
the launcher with the same search contract as `LauncherIndex::search`, and a
control centre whose tiles **name what they would bind to**
(`NetworkManager`, `PipeWire`, `xdg-desktop-portal`) instead of pretending to
work.

It composites in the same order the compositor does — wallpaper, windows,
chrome — so a screenshot and a `tahoe-preview` render can be compared
directly.

## 6. What is verified, and what is not

**Verified on this machine:**
- the tokens are generated and deterministic; a Rust test parses the emitted
  Swift back and compares every value;
- `tahoe-compat apple toolchain` reports the missing prerequisite correctly
  and distinguishes "no Swift" from "no SDK";
- `tahoe-compat apple project apple` reads the real committed project;
- `tahoe-compat apple build apple --dry-run` prints the exact command;
- `tahoe-compat apple inspect` correctly reads a bundle with xtool's shape:
  the iOS layout, `MinimumOSVersion`, an arm64 Mach-O, `PLATFORM_IOS`, an
  unsigned binary, and the three framework dependencies.

**Not verified:**
- **no build has been attempted.** Swift 6.4 and the iOS SDK are not
  installed, and the SDK needs an Apple ID.
- **no Swift file has been compiled.** A typo in a view body would not have
  been caught. The token target is the only part with a compile-time
  guarantee, and that guarantee is structural, not tested.
- **the generated `Info.plist` has not been compared to a real one.** The
  fixture is modelled on xtool's `Packer.swift`, not captured from a run.
- **the Swift test target has never been run.**

## 7. Why this belongs in Tahoe

A distribution that ships a Wayland desktop and a design system should be
able to answer "does this look right on my phone, too?" without a second
machine and a second source of truth for the palette. That is what this
directory is: the same tokens, the same layout metrics, the same empty
states and caveats — rendered by a different toolkit, on a different OS,
built from the same repository on Linux.
