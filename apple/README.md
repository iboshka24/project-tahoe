# Tahoe Gallery — SwiftUI app built on Linux

A real SwiftUI application that renders the Tahoe design system on Apple
platforms, buildable from Arch Linux with [xtool](https://github.com/xtool-org/xtool)
and no Mac.

**Status: the source is complete and the tokens are generated from the Rust
design system. It has not been built, because the build needs two things
this repository does not ship** — see "What you have to provide".

---

## Why this exists

Tahoe has two front ends that must look identical:

| Front end | Language | Renders via |
|---|---|---|
| The Wayland shell | Rust | `tahoe-shell::preview`, `tahoe-compositor` |
| This application | SwiftUI | the iOS/macOS SDK |

Two hand-written copies of a palette diverge, so the Rust
[`DesignSystem`](../crates/tahoe-shell/src/design.rs) is the single source of
truth and `TahoeDesign.swift` is **generated** from it:

```sh
cargo run -p tahoe-shell --bin tahoe-design-export -- \
    apple/Sources/TahoeDesign/TahoeDesign.swift
```

A test in `tahoe-shell` regenerates the file and fails if the committed copy
has drifted, so editing the Swift by hand produces a failing build rather
than a silent difference. There is a second check that runs the other way:
`Tests/TahoeDesignTests` asserts the numbers from the Swift side.

## What you have to provide

xtool does not download the SDK for you, and neither does this repository.
Apple's SDK is not redistributable here.

1. **A Swift 6.4 toolchain** for Linux, from
   <https://swift.org/install/linux>. Extracted to a directory — no root
   needed; just put its `usr/bin` on `PATH`.
2. **An `Xcode.xip`**, downloaded from
   <https://developer.apple.com/download/all/?q=Xcode> while signed in with
   your Apple ID. That is roughly seven gigabytes. xtool extracts the iOS SDK
   from it locally and installs a Swift SDK named `darwin`.
3. **[xtool](https://github.com/xtool-org/xtool)** itself — an MIT-licensed
   AppImage, no root needed either.

The Xcode SDK is governed by Apple's license. xtool's documentation says you
must sign in and accept it yourself; whether the extracted SDK may be
redistributed is not addressed anywhere I could verify, so **do not assume it
may be**. Keep it on your own machine.

## Build

```sh
# Once, and it asks for the Xcode.xip path:
xtool sdk install /path/to/Xcode.xip
swift sdk list            # should list `darwin`

cd apple
xtool dev build           # writes ./xtool/TahoeGallery.app
```

`xtool dev build` needs **no Apple developer account**. Signing only happens
with `--sign` or when entitlements are configured, and neither is done here.

The result is a real `.app` bundle: an arm64 Mach-O executable and an
Info.plist. That is exactly the shape `tahoe-compat`'s inspection code
parses, which is why this directory doubles as a source of real test
fixtures:

```sh
cargo run -p tahoe-compat -- mac bundle apple/xtool/TahoeGallery.app
cargo run -p tahoe-compat -- mac macho \
    apple/xtool/TahoeGallery.app/TahoeGallery
```

## Run

There is no simulator on Linux. Running the app needs a real iPhone or iPad
connected over USB, with Developer Mode enabled and xtool's certificate
trusted on the device:

```sh
xtool dev run             # requires `xtool auth` first
```

This is the honest limit of the whole approach: **you can build an iOS app on
Linux, and you cannot run one there.** An app built this way cannot run under
Darling either — it links UIKit, which Darling does not implement.

## Layout

```
apple/
├── Package.swift                     SwiftPM manifest (xtool needs a library)
├── xtool.yml                         project configuration
├── Sources/
│   ├── TahoeDesign/TahoeDesign.swift GENERATED — do not edit
│   └── TahoeGallery/
│       ├── TahoeGalleryApp.swift     @main
│       ├── ContentView.swift         the gallery
│       ├── Model/GalleryModel.swift  state; mirrors the Rust shell model
│       └── Views/                    Panel, Dock, Launcher, Window, Control Centre
├── Tests/TahoeDesignTests/           token assertions, runnable on Linux
└── reference/Info.plist              the keys xtool writes, for our parsers
```

## What is verified, and what is not

**Verified without the toolchain:**
- the tokens compile-check by construction (generated, deterministic);
- the Rust and Swift sides are compared by a Rust test that parses the
  emitted values back;
- `Tests/TahoeDesignTests` asserts the same numbers from Swift.

**Not verified:**
- no build has been attempted; Swift 6.4 and the iOS SDK are not installed;
- the SwiftUI views have never been compiled, so a typo in a view body would
  not have been caught;
- the generated `Info.plist` has not been compared against a real one. The
  fixture in `reference/` is modelled on xtool's `Packer.swift`, not captured
  from a run.
