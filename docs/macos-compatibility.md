# macOS compatibility: what Tahoe actually provides

> Blunt version: Tahoe inspects macOS software thoroughly and runs almost
> none of it. Running it is Darling's job, and Darling's own maintainers
> describe GUI support as in-progress.
>
> Status: 2026-10-14. **No Mach-O binary has been executed on any machine
> used to develop this.** Darling is not installed.

---

## 1. The eight parts of the problem, separated

The mission asked for these to be separated. They are, and the table is the
most useful thing in this document:

| # | Part | Who does it | Status |
|---|---|---|---|
| 1 | Detect and read a `.dmg` | **Tahoe** | implemented, tested |
| 2 | Safely mount or extract it read-only | external tool | designed, not implemented |
| 3 | Find the `.app` bundle | **Tahoe** | implemented, tested |
| 4 | Read Mach-O and universal binaries | **Tahoe** | implemented, tested |
| 5 | Check architecture and dependencies | **Tahoe** | implemented, tested |
| 6 | Run it through a Darwin runtime | **Darling** | not installed |
| 7 | Supply the system libraries and frameworks | **Darling** | partial, upstream |
| 8 | GUI, graphics, audio, input, shell integration | **Darling** + Tahoe | mostly upstream, not ready |

Tahoe's contribution is parts 1, 3, 4, 5, and the shell integration half of
8. Parts 2 and 6 need tooling that is not installed. Part 7 is where the
honesty lives.

## 2. What is implemented

### 2.1 DMG: parsed properly, never executed

The 512-byte `koly` trailer is parsed field by field: version, both forks,
segment number and count, the XML plist pointer, the code-signature blob
pointer, and the sector count (from which the decoded size follows). **Every
field is big-endian**, which is tested explicitly.

The trailer's XML pointer is followed to read the real `blkx` block table,
which yields the **compression methods the image actually uses**. That
matters because LZFSE needs Apple's decompressor while zlib, bzip2 and LZMA
do not — so a user is told which one they have before being told to mount
something.

A DMG produces `Strategy::Inspect`, never a command. The hint reports split
segments ("segment 2 of 3, so all segments are needed"), decoded size, and
compression methods.

### 2.2 Mach-O: read thoroughly

Load commands parsed: `LC_LOAD_DYLIB`, `LC_LOAD_WEAK_DYLIB`,
`LC_REEXPORT_DYLIB`, `LC_ID_DYLIB`, `LC_RPATH`, `LC_LOAD_DYLINKER`,
`LC_UUID`, `LC_MAIN`, `LC_CODE_SIGNATURE`, `LC_VERSION_MIN_MACOSX`,
`LC_BUILD_VERSION`. Both endiannesses, 32- and 64-bit headers. A load
command that runs past the buffer is a typed error, not a walk off the end.

Dependency resolution implements `dyld(1)`'s three roots and **reports every
candidate path in order** rather than picking one, so a caller can choose
the first that exists. Weak dependencies are excluded from the
required-to-resolve set. An `@rpath` dependency with no rpaths is reported
**unresolved**, not assumed fine.

### 2.3 Universal binaries, and the Java collision

The `fat_arch` table is parsed into per-slice architecture, offset, size and
alignment. Slice selection prefers the requested architecture, then any
64-bit slice.

`arm64e` is correctly distinguished from `arm64` (it is `cputype`
`CPU_TYPE_ARM64` with `cpusubtype` 2, and Apple packs capability bits into
the subtype) and from `arm64_32` (`0x0200000C`, 64-bit registers with 32-bit
pointers, used by watchOS). An earlier version of this code got that wrong
and a test caught it.

`0xCAFEBABE` is also the Java class magic. `tahoe-format` resolves it by
`nfat_arch` plausibility plus a validity check on each slice's magic — the
rule `file(1)` uses. When neither reading holds, the result is `Ambiguous`
and the file is refused, because a wrong guess sends a Java class to a Mach-O
loader.

### 2.4 `.app` bundles

Resolution reads `Contents/Info.plist` and requires `CFBundleExecutable`.
It **refuses** rather than guessing when that key is absent: picking the
largest file in `Contents/MacOS` would work most of the time and be wrong
the rest. It also reports `CFBundleIdentifier` (dock identity),
`LSMinimumSystemVersion`, `CFBundleIconFile`, `LSUIElement` (agent apps) and
**every** architecture present in the executable.

### 2.5 Property lists, both encodings

`bplist00` is parsed directly — object table, offset table, 32-byte trailer,
extended lengths, UTF-16 strings, arrays, dicts — with a depth guard so a
self-referential plist terminates. XML plists are parsed for the subset an
`Info.plist` uses.

The alternative was `libplist` (LGPL-2.1). That was rejected: the format is
small, and a dependency for reading at most a dozen keys is not worth the
update surface.

### 2.6 Containers

XAR (`xar!`, the format behind `.pkg` and `.xip`) is identified, and the
structure is documented: a `.pkg` `Payload` is a cpio stream wrapped in gzip
or `pbzx`; a `.xip` holds a `pbzx` stream of XZ chunks that decode to cpio.
Neither is unpacked by Tahoe; both are refused as executables.

## 3. What Darling actually provides

Verified from the source, not from marketing:

- `mldr` maps the executable and whatever `LC_LOAD_DYLINKER` names, then
  jumps to *its* entry point. **It bootstraps dyld; it does not bypass it.**
- `darlingserver` is a userspace "kernel" over
  `$DPREFIX/.darlingserver.sock`, implementing ~80 Mach and BSD calls. It
  replaced the kernel module in **July 2022** — so there is no DKMS module
  to maintain, and none has been needed for four years.
- The prefix is an overlayfs merge, which is why it cannot live on NFS or an
  encrypted home directory.
- `darling` has **no options** beyond `--help` and `--version`. The grammar
  is `darling <program> [args]`, `darling shell`, `darling exec`,
  `darling shutdown`, with `DPREFIX` as the only variable.
- Darling **does** ship a `binfmt.d` template, but its interpreter path is
  **inconsistent in-tree**: CMake installs `mldr` to
  `libexec/darling/usr/libexec/darling/mldr` while the template points at
  `libexec/darling/bin/mldr`, and nothing creates `bin/mldr`. A distro must
  fix that. Tahoe's own rules point at its own dispatch helper instead.

**In the maintainers' words:** "many CLI tools already functional. GUI
application support is in active development."

**Hard limits, all from the source:**

| Limit | Evidence |
|---|---|
| x86_64 only | `mldr.c` contains `#error TODO: aarch64`; the ARM64 PR is open and the maintainer has said he will not merge it |
| Xcode does not run | README says so explicitly |
| `Combine` missing | open issue #1215 |
| `CryptoKit` missing | open issue #1610 |
| SwiftUI is a stub | PR adding stubs, closed |
| Metal is early and separate | `darling-metal` is a different repo |
| No automated end-to-end evidence | the only CI workflow builds DEBs and never executes a Mach-O |

## 4. What Tahoe will and will not claim

**Will claim:** Tahoe can identify a macOS container, find the application
inside it, read its metadata, list its architectures, parse its dependencies
and say exactly what it found.

**Will not claim:** that any `.app` works. Not even a CLI tool. That
requires Darling installed and a measurement per application.

The confidence model enforces this: a macOS application always gets
`Confidence::Layer` with the caveat that Darling's GUI support is partial,
even when every check passes.

## 5. What has NOT been verified

- **No Mach-O has been executed.** Darling is not installed.
- No DMG has been mounted or extracted. The trailer and block table are
  parsed; nothing is decompressed.
- No APFS image has been handled. **UNVERIFIED** whether typical modern DMGs
  are APFS — the format is not in mainline Linux either way.
- No real `.app` from a real macOS system has been resolved. The fixtures
  are built from the documented bundle layout.
- Darling's `darling <program>` invocation is **taken from its README**, not
  observed. The code says so.
- HFS+ and APFS parsing are **not implemented**: both are delegated to
  external tools rather than reimplemented.

## 6. The smallest honest vertical slice

If Darling were installed, the first thing worth testing is exactly this,
and nothing more ambitious:

1. Take a `.dmg` containing a CLI tool.
2. Parse its trailer, mount it read-only with an external tool.
3. Find the `.app`, resolve `CFBundleExecutable`.
4. Parse the Mach-O, list its architectures, check it is x86-64.
5. Run it through Darling and record the exit code.

Steps 1, 3 and 4 are already implemented and tested. Step 2 needs a mount
tool. Step 5 needs Darling. A GUI application is not in this slice and
should not be promised in one.
