# Project Tahoe

An Arch Linux-based distribution with a macOS-Tahoe-styled Wayland desktop and a
single execution model for Linux ELF, Windows PE, macOS Mach-O/.app, AppImage
and DMG-delivered applications.

**Where things stand:** [`docs/STATUS.md`](docs/STATUS.md) — what is
confirmed by running it, what is written but unverified, and what does not
work. Start there.

**Status: foundation plus.** The architecture, tooling and seven working
crates exist; their tests pass and the shell renders real scenes with real
text (see `TODO.md` for what is still ahead). There is no runnable desktop
session built from the crates yet — the renderer is not yet a Wayland
client — but the archiso profile now ships a complete live desktop instead:
[Hyprland with a vendored MacShell session](https://github.com/0-ss/MacShell)
(`docs/iso-session.md`). The ISO itself is still not built. See
[ROADMAP.md](ROADMAP.md) for what that means and what comes next.

---

## Read this first

Three things about Tahoe are easy to get wrong, so they are stated up front:

1. **It is not a compatibility guarantee.** Identifying a file as `Mach-O` and
   handing it to Darling is not "macOS apps work on Tahoe". Darling's own
   maintainers describe GUI AppKit support as minimal. Tahoe reports a
   *confidence* for every execution decision instead of a promise.
2. **Wine is not Windows, Darling is not macOS.** They are translation layers
   with documented, measurable gaps. See `ARCHITECTURE.md` §6.
3. **DMG is never executed.** It is a disk image. It is mounted and extracted.
   So is `.app` browsing, at the bundle level.

---

## The one idea

One session, one Dock, one file manager — and an honest answer to *"what
happens if I double-click this?"* for every kind of binary the user might
actually own.

```
                    ┌─────────────────────────────┐
   /Volumes/App.dmg │  tahoe-uel                  │
   foo.exe          │  probe → strategy → spawn   │──► native exec()
   Foo.app/…/Foo    │  …with a confidence report  │──► wine …            (Wine 11)
   program.AppImage │                             │──► darling …         (Darling)
   ./configure      │                             │──► /bin/sh …
                    └─────────────────────────────┘
```

---

## What is real today

| Component | State | Evidence |
|---|---|---|
| `tahoe-core` | ✅ implemented | `cargo test -p tahoe-core` |
| `tahoe-format` | ✅ implemented, 40+ tests | `cargo test -p tahoe-format` |
| `tahoe-uel` | ✅ implemented, pure decision logic | `cargo test -p tahoe-uel` |
| `tahoe-compositor` | ✅ compiles against Smithay 0.7 | `cargo build -p tahoe-compositor` |
| `tahoe-shell` | ✅ model implemented (dock/workspaces/launcher index) | `cargo test -p tahoe-shell` |
| `tahoe-diagnostics` | ✅ read-only probes | `cargo test -p tahoe-diagnostics` |
| `iso/` archiso profile + MacShell live session | 🟡 written, **not yet built** | structural checks: `tests/check-iso.sh`; build: `iso/build.sh` / CI (`docs/iso-session.md`) |
| `packages/` PKGBUILDs | 🟡 written, **not yet built** | needs `makepkg`/chroot |
| `binfmt.d/` entries | 🟡 shipped as files, **not registered** | requires root + consent |
| Tahoe Shell GUI | 🔴 not started | see ROADMAP |

Nothing in this repository claims a successful ISO build, a successful Wine
run, or a successful Darling run, because none of those have been performed
here. `docs/research/` records exactly what was verified and what was not.

---

## Quick start (development)

```sh
# Toolchain: Rust >= 1.87 — MSRV of Smithay 0.7 (see `rust-version` in Cargo.toml).
# No rustup pin file: the reference machine uses system cargo (1.99.0).
cargo test                        # all crates
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check

# optional: the format probe CLI
cargo run -p tahoe-uel -- probe /bin/ls
cargo run -p tahoe-uel -- probe /path/to/installer.exe
```

Reference environment used for verification (CachyOS 6.18, Rust 1.99.0):
see [`docs/research/2026-10-13-smithay-build-verification.md`](docs/research/2026-10-13-smithay-build-verification.md).

---

## Repository map

```
crates/   Rust workspace — compositor, UEL, format parser, shell model
iso/      archiso profile — Hyprland + vendored MacShell live session
          (docs/iso-session.md, provenance in iso/macshell-vendor.md)
packages/ PKGBUILDs (sources 0BSD, per Arch policy)
binfmt.d/ binfmt_misc entries we ship
fixtures/ synthetic binaries for the format tests + generator
docs/     research findings, ADRs, testing strategy
```

Full detail in [ARCHITECTURE.md](ARCHITECTURE.md).

---

## Licensing

First-party code: **`MIT OR Apache-2.0`**.

The interesting question is Darling, which is `GPL-3.0-or-later`. Tahoe keeps
it out of every first-party binary: compatibility layers are spawned as
separate processes and talked to over pipes/D-Bus. The full analysis — including
why this is both the legally safe boundary and the right engineering choice —
is in [LICENSING.md](LICENSING.md).

---

## Contributing

[CONTRIBUTING.md](CONTRIBUTING.md). Short version: keep the layer boundaries,
keep the honesty guarantees, add a test for every format claim you make.
