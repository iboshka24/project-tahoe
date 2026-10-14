# Compatibility matrix

> Every row states what is **implemented and tested**, by what, and what it
> still cannot do. Nothing here is a promise about a specific application.
>
> Status: 2026-10-14, kernel 6.18.55, 457 tests passing.

---

## 1. Recognised formats

| Format | Detected by | Parsed fields | Tests | Can it be executed? |
|---|---|---|---|---|
| ELF 32/64, LE/BE | `\x7fELF` | class, endianness, OSABI, `e_type`, `e_machine` | 13 | **yes**, natively |
| PE32 / PE32+ | `MZ` + `e_lfanew` + `PE\0\0` | COFF machine, PE32 vs PE32+, subsystem | 8 | via Wine (not installed here → refused honestly) |
| AppImage type 2 | ELF + `AI\x02` at offset 8 | — | 2 | natively, but marked **unsandboxed** |
| Mach-O 32/64, both endiannesses | 4 magics | `cputype`, `filetype`, `MH_PIE`, `LC_*` load commands | 15 | via Darling (not installed here → refused) |
| Mach-O universal | `cafebabe` / `cafebabe64` / `bebafeca` | full `fat_arch` table with per-slice arch, offset, size, align | 9 | the selected slice, via Darling |
| Java `.class` | `cafebabe` + `nfat_arch ≥ 20` | — | 4 | **never** — it is not a program on Linux |
| UDIF `.dmg` | `koly` trailer (big-endian, 512 bytes) | version, forks, segments, XML pointer, signature blob, sector count, decoded size | 8 | **never** — inspect only |
| DMG block table | base64 `blkx` in the embedded plist | distinct compression methods per run | 5 | n/a |
| XAR (`.pkg`, `.xip`) | `xar!` | header size, version | 2 | **never** — unpack only |
| OLE2 / `.msi` | `D0CF11E0A1B11AE1` | byte order, sector shift | 2 | via `wine msiexec` (needs Wine) |
| Plist (XML, `bplist00`) | header / `<plist` | full object graph: strings, ints, reals, bools, arrays, dicts, data, UTF-16 | 14 | **never** — read only |
| `.app` bundle | directory + plist, **both layouts** | `CFBundleExecutable`, identifier, minimum OS (`LSMinimumSystemVersion` *or* `MinimumOSVersion`), icon, `LSUIElement`, package type, all slice architectures | 12 | the **inner executable**, via Darling (macOS only) |
| iOS `.app` (flat) | `Foo.app/Info.plist` | as above; `PLATFORM_IOS` reported as such, never as a macOS minimum | 4 | **not executable on Linux** — it links UIKit |
| Script | `#!` | interpreter **and its arguments** | 6 | yes, via the interpreter |
| Windows `.lnk` | 76-byte header + CLSID | flags, file attributes, name, relative path, working dir, arguments | 6 | n/a — used for discovery |

**Not implemented:** APFS containers, HFS+ parsing (both are delegated to
external tools rather than reimplemented), `.xip` pbzx decoding,
`bplist01`/`bplist02`, sparse `LC_DYLD_CHAINED_FIXUPS` binding.

## 2. Architectures

| Situation | Outcome | Where |
|---|---|---|
| ELF matching the host | `Strategy::Native`, `Confidence::Verified` | tested |
| ELF with unknown architecture | `Native` — a truncated header must not block a real binary | tested |
| ELF for a foreign architecture, no emulator | refused, naming the needed arch | tested |
| ELF for a foreign architecture, emulator present, speculative allowed | `Emulator`, `Confidence::Speculative` | tested |
| x86-64 PE on x86-64 | Wine | tested |
| x86 PE on x86-64 | Wine — **new WoW64 needs no multilib** | tested |
| arm64 PE on x86-64 | refused or `Emulator` with an explicit opt-in | tested |
| x86-64 Mach-O on x86-64 | Darling | tested |
| arm64 Mach-O | **refused**: `mldr.c` has `#error TODO: aarch64` | tested |
| arm64-only `.app` | refused with "darling only supports x86_64 today" | tested |
| Universal binary containing x86-64 | the x86-64 slice is selected and reported | tested |

## 3. Strategy selection

| Input | Strategy | Confidence | Notes |
|---|---|---|---|
| Native ELF | `Native` | `Verified` | |
| PE | `Wine { prefix }` | `Layer` + caveat | refuses if Wine is absent, naming it |
| `.msi` | `WineInstaller` | `Layer` + caveat | distinct: a *different program* with different arguments |
| Mach-O | `Darling { prefix }` | `Layer` + caveat | |
| `.app` | `MacBundle { bundle, executable }` | `Layer` + caveat | runs the inner executable, never the directory |
| Script | `Script { interpreter, args }` | `Plausible` | arguments from the shebang are preserved |
| AppImage | `Native` only with `allow_speculative` | `Speculative` | refused by default; the caveat says it is unsandboxed |
| Foreign arch + emulator | `Emulator` | `Speculative` | only with an explicit opt-in |
| `.dmg` | `Inspect { Dmg, hint }` | `Verified` | the hint reports split segments and compression methods |
| XAR / plist | `Inspect` | `Verified` | |
| Ambiguous bytes | **refused** | `Refused` | never guessed |
| Unknown bytes | **refused** | `Refused` | |

## 4. Wine integration

| Capability | Status | Evidence |
|---|---|---|
| Prefix creation/update invocation | environment + flags built | `launch_env` tested |
| Per-application isolated prefixes | implemented | `Prefix::for_application`, sanitisation tested against traversal |
| Architecture detection (WoW64 vs 64-only) | implemented | tested both shapes |
| Start Menu discovery (3 layouts) | implemented | tested |
| Nested vendor folders | implemented, depth-capped at 6 | tested |
| `.lnk` parsing | header + StringData | tested, including hostile lengths |
| Suffix cleanup ("- Shortcut", "Uninstall") | implemented | tested |
| `winemenubuilder` `.desktop` parsing | `Name`, `Exec`, `Icon`, `StartupWMClass`, `NoDisplay` | tested |
| Multi-prefix separation | entries from another prefix are refused | tested |
| dedup across both sources | by launch target | tested |
| Shell-free `Exec` parsing | spec-compliant quoting, no evaluation | tested with `$(touch …)` |
| NTSync detection | device + kernel version, reported separately | tested |
| **Actually running a PE** | **not tested** | Wine is not installed on any machine used |

`WINEESYNC` and `WINEFSYNC` are deliberately absent: they are not upstream
Wine variables. A test asserts the documented list does not contain them.

## 5. macOS integration

| Capability | Status | Evidence |
|---|---|---|
| Bundle layout detection (macOS vs iOS) | implemented | 3 tests, including a cross-layout test |
| xtool prerequisite detection | implemented, ordered so the SDK is not requested before Swift | 7 tests |
| `xtool.yml` parsing | implemented | 6 tests, against the committed file |
| Apple build command construction | implemented, never `--sign` | 4 tests |
| Inspecting an xtool-shaped iOS bundle | implemented | 6 tests |
| Generated Swift compiles | **verified** | `swift build --target TahoeDesign` |
| Swift syntax of all files | **verified** | `swiftc -parse`, needs no SDK |
| Swift token + model tests | **verified**, 21 passing | `swift test` |
| **Building an iOS app** | **not attempted** | needs an Apple-ID-gated SDK |
| Mach-O load command parsing | `LC_LOAD_DYLIB`/`WEAK`/`REEXPORT`, `LC_ID_DYLIB`, `LC_RPATH`, `LC_LOAD_DYLINKER`, `LC_UUID`, `LC_MAIN`, `LC_BUILD_VERSION`, `LC_VERSION_MIN_MACOSX`, `LC_CODE_SIGNATURE` | 15 tests |
| Universal binary slice selection | preference order, then any 64-bit | tested |
| `arm64e` vs `arm64_32` | distinguished by `cpusubtype` capability bits | tested |
| `@rpath`/`@loader_path`/`@executable_path` | resolved, all candidates reported in order | tested |
| Unresolved dependency reporting | against the real filesystem; weak deps excluded | tested |
| iOS-only binary | reported as having **no** macOS minimum, not faked | tested |
| `.app` resolution | `CFBundleExecutable` only; refuses rather than guessing | tested |
| Bundle with no executable key | refused | tested |
| Bundle with a missing executable | refused, naming the path | tested |
| Agent apps (`LSUIElement`) | reported | tested |
| Binary plist | full object graph incl. extended lengths, UTF-16, self-reference guard | 8 tests |
| XML plist | dict/array/string/int/real/bool/data, entity unescaping, depth cap | 6 tests |
| **Actually running a Mach-O** | **not tested** | Darling is not installed, and Darling itself has no CI that executes one |

## 6. Kernel-level dispatch

| Capability | Status | Evidence |
|---|---|---|
| Build and validate register strings | implemented; every kernel constraint checked | 12 tests |
| Rootless registration in a user namespace | **works** | end-to-end, on 6.18.55 |
| Kernel dispatches a file to our interpreter | **works** | `binfmt self-test` |
| Namespaced entries do not escape | **verified** | host dispatcher compared before/after |
| Entry visible inside its namespace | **verified** | |
| Host untouched | **verified** | |
| `T` / `L` / `D` / BPF `B` | not used — 7.3 only | documented, with the reasoning |
| System-wide `binfmt.d` files | shipped as files; not installed by tests | needs root |

## 7. Supervision

| Capability | Status | Evidence |
|---|---|---|
| No shell, ever | argv straight to `execve` | `$(touch …)` argument stays literal |
| Environment hygiene | `env_clear` + 22-variable whitelist | leak-canary test |
| Explicit overrides beat automatic ones | plan then policy | tested |
| Timeout | blocking wait in a thread, exact deadline | 150 ms timeout kills a 30 s sleep |
| Process-group kill | `process_group(0)` + `killpg` | **grandchild does not survive** |
| Bounded capture | cap reported, writer not killed by EPIPE | tested |
| Structured log | one JSON line per launch | tested, including escaping |
| Measured overhead | ~1.5 ms per launch | `tahoe-bench` |

## 8. What is not supported, and will not be

- Windows kernel-mode drivers, anti-cheat with a kernel component, WDDM,
  DirectStorage. Wine has no kernel; this is not a Tahoe gap.
- Running arm64 Mach-O. Darling upstream does not.
- Running Xcode. Darling's own README says so.
- SwiftUI / Combine / CryptoKit applications. Missing or stubbed upstream.
- Metal. The Vulkan-backed implementation is a separate early project.
- APFS disk images with in-kernel tooling; read-only FUSE only.
- Any claim that a *specific* application works. That requires measurement
  per application, which is the point of the confidence model.

## 9. Test totals

| Crate | Tests |
|---|---|
| `tahoe-core` | 7 |
| `tahoe-format` | 67 |
| `tahoe-uel` (decisions + supervisor) | 68 |
| `tahoe-compat` (binfmt, wine, macos, xtool) | 148 |
| `tahoe-compositor` | 80 |
| `tahoe-shell` | 133 |
| `tahoe-diagnostics` | 8 |
| **Total** | **511** |

Plus, outside `cargo test`:

- `wayland-info` against the running compositor;
- `tests/verify-preview.py` — 11 independent visual assertions;
- `swift test` — 21 Swift tests, and `swiftc -parse` over 11 files;
- the `tahoe-bench` harness.

`tests/check-all.sh` runs all of it: **15 checks pass, 0 fail, 1 skipped**
(the skip being the Apple-ID-gated iOS SDK).
