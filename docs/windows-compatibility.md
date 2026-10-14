# Windows compatibility: what Tahoe actually provides

> The honest version. Wine is a compatibility layer, not Windows, and this
> document says so everywhere it matters.
>
> Status: 2026-10-14. **No Windows program has been executed on any machine
> used to develop this.** Wine is not installed, and installing it needs
> root. Everything below is therefore of two kinds, labelled explicitly:
> code-and-test evidence, and upstream-documented behaviour.

---

## 1. The claim, stated precisely

Tahoe provides **integrated dispatch and discovery** for Windows software.
It does not provide, and cannot provide, Windows compatibility: that is
Wine's, and it is partial.

| Layer | Whose job | Status |
|---|---|---|
| Recognise a PE, an MSI | Tahoe | implemented, tested |
| Decide whether running it is wise | Tahoe | implemented, tested |
| Supply the prefix, environment and supervision | Tahoe | implemented, tested |
| Discover installed programs and publish them to the Dock | Tahoe | implemented, tested |
| **Make the program work** | **Wine** | whatever Wine does, no more |
| Kernel drivers, anti-cheat, WDDM, DirectStorage | **nobody** | not possible |

## 2. What is implemented

### 2.1 Identification

- `MZ` + `e_lfanew` + `PE\0\0`, COFF machine, PE32 vs PE32+, subsystem.
  Verified against generated fixtures and real system binaries.
- An `.msi` is recognised as an **OLE2 compound file** (`D0CF11E0A1B11AE1`),
  not by its extension. A self-extracting PE stub named `setup.msi` is
  correctly identified as a PE and sent to Wine as a program instead.

### 2.2 Dispatch

| Input | Action |
|---|---|
| PE | `wine <program>` with `WINEPREFIX` and `WINEDLLOVERRIDES="mscoree,mshtml="` |
| MSI | `wine msiexec /i <package>` |
| Kernel-mode driver, anti-cheat | not attempted; there is nothing to attempt it with |

`WINEDLLOVERRIDES="mscoree,mshtml="` is the de-facto way to stop Wine
popping the Gecko/Mono download dialog during a first-run or headless
launch. It is *not* a documented upstream constant, and the code says so.

### 2.3 Prefix management

- `Prefix::for_application(name)` creates an isolated prefix per
  application. Isolation is the containment boundary: a broken registry, a
  failed installer, or a hostile setup program is confined to one directory.
- Names are sanitised. A traversal attempt is **refused**, not cleaned up: a
  half-cleaned `../../etc` is exactly the kind of value that later looks like
  a path.
- Architecture is read from disk (`system32` + `syswow64` present means
  WoW64), not assumed.

### 2.4 Discovery

Two sources, because neither is complete alone:

1. **`winemenubuilder`'s `.desktop` files** in
   `$XDG_DATA_HOME/applications/wine/**`. Tahoe parses `Name`, `Exec`,
   `Icon`, `StartupWMClass` and `NoDisplay`. `StartupWMClass` is what makes
   a Wayland compositor able to group a window with its dock entry, and
   reimplementing the generator would be strictly worse than reading it.
2. **Start Menu shortcuts** under the three layouts Wine produces
   (`AppData\Roaming\...`, `ProgramData\...`, legacy
   `users/<user>/Start Menu/...`), parsed as MS-SHLLINK for the header and
   `StringData`.

Entries are deduplicated by launch target, and an entry naming a different
prefix is refused so a multi-prefix system does not cross-match.

**Why `.lnk` rather than resolving the target:** a shortcut is launched with
`wine start`, which resolves it the way Windows does, including arguments and
working directory. Resolving a shell item to a path requires the shell
namespace; `start.exe` already has it.

### 2.5 Environment

`env_clear()` plus a 22-variable whitelist. A child does not inherit
`LD_PRELOAD`, `PYTHONPATH`, `SSH_AUTH_SOCK` or anything else it has no
business seeing. There is a leak-canary test.

## 3. What upstream Wine provides (verified, not tested here)

- **New WoW64** (complete in Wine 11.0): 32-bit PE thunks into a 64-bit
  process through `wow64.dll`/`wow64win.dll`/`wow64cpu.dll`. **No 32-bit
  host libraries and no `[multilib]`** are needed. Arch removed that
  dependency in June 2025.
- **NTSync** (`/dev/ntsync`, Linux 6.14+) is used automatically by Wine 11.
  There is no environment variable. Tahoe reports whether the device exists
  and whether the kernel is new enough **separately**, because "upgrade the
  kernel" and "load the module" are different instructions.
- **`WINEESYNC` and `WINEFSYNC` do not exist in upstream Wine.** They are
  Proton/wine-staging only. A test asserts Tahoe's documented list does not
  contain them.
- **The Wayland driver exists** (`dlls/winewayland.drv/`) but X11 has
  precedence in the built-in driver list, and Wine 11.0's own release notes
  still call it experimental.
- **Gecko and Mono** are looked up in `/usr/share/wine/{gecko,mono}/` and
  five other places before any download is attempted.
- **`winemenubuilder`** writes `.desktop`, MIME, menu and icon files, and
  can be disabled with a documented registry override.

## 4. Why Wine is not Windows

From Wine's own documentation:

> "Wine is not a virtual machine, it does not emulate a CPU, and you are not
> supposed to install Windows nor any Windows device drivers on top of it;
> rather, Wine is an implementation of the Windows API."

Concretely:

| Missing | Consequence |
|---|---|
| `ntoskrnl`, HAL, kernel-mode drivers | `.sys` files are placeholders. Kernel anti-cheat cannot load. |
| WDDM | No hardware Windows display driver model; Wine has its own user-mode drivers. |
| Real syscall instruction interface | The NT syscall interface is emulated in userspace (Wine 11 matches recent Windows syscall *numbers* so hardcoded callers work). |
| NT registry hives | Emulated as flat text (`system.reg`, `user.reg`) in the prefix. |
| DirectStorage | Not implemented upstream. |
| Copy protection | Wine's stated position: it cannot and will not break it. |

## 5. What has NOT been verified

This list exists so nobody mistakes the matrix for a compatibility database:

- **No PE file has been executed.** Not once. The supervisor path is tested
  with `/bin/sh`, `sleep`, and small scripts.
- No prefix has been created, because that needs Wine.
- No installer has been run.
- No `.lnk` produced by a real Windows installer has been parsed. The
  fixtures are synthesised from the MS-SHLLINK layout, and the header,
  `LinkTargetIDList` skip, `LinkInfo` skip and `StringData` walk are all
  tested — but against bytes we wrote.
- `msiexec`'s actual behaviour on a real `.msi` is unverified.
- The `WINEDLLOVERRIDES="mscoree,mshtml="` idiom is de-facto, not documented.

## 6. What a user should expect

- A Windows program in the Dock, with an icon, launching on click.
- An honest refusal when Wine is not installed, naming Wine.
- A caveat on every Windows entry: *"runs under Wine: a compatibility layer,
  not Windows"*.
- No promise that any given program works. `winetricks`, `wine-staging` and
  Proton exist precisely because that promise cannot be made.

## 7. The next honest step

Install Wine on a test machine (`pacman -S wine`) and run the one test that
has never run: `tahoe-uel run something.exe`. Everything up to the `execve`
is already verified; the missing evidence is entirely on Wine's side.
