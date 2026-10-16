# Proton: Valve's game-oriented Wine, integrated

> Researched from upstream (`ValveSoftware/Proton` @ `proton_11.0`,
> `CachyOS/proton-cachyos`, `GloriousEggroll/proton-ge-custom`) on
> 2026-10-16, then **actually run** against a real Windows PE.
>
> Result: `tahoe-uel run game.exe` → `Strategy::Proton` → exit 0.

---

## 1. The correction that shapes the design

**Proton is not a better Wine. It is a different Wine for a different
job.** Asking for it to replace Wine everywhere would make the desktop
worse, and the evidence is in its own source:

- `proton run` does not execute your program. It executes
  `c:\windows\system32\steam.exe` — a stub — with your program as an
  argument.
- The prefix is seeded with Steam client DLLs, and `steam.exe` is forced to
  a builtin override.
- `WINEDEBUG=-all` is set, `winebth.sys` is disabled, `opencl=n,d` is
  forced, and a per-appid workaround table is applied.
- Its own FAQ calls launching an exe manually "untested".

For a **game** that machinery is the difference between working and not.
For a **spreadsheet** it can only hurt. So Tahoe does not choose globally —
it chooses **per program, from the program's import table**.

```sh
$ tahoe-uel probe hello.exe     # KERNEL32, SHELL32, COMCTL32
strategy=Wine { prefix: "…" }

$ tahoe-uel probe game.exe      # KERNEL32, d3d11
strategy=Proton { build: "…/proton-cachyos-11.0-20261005-slr-x86_64", … }
confidence=layer:proton
summary=…imports ["kernel32.dll", "d3d11.dll"], so it is a game or 3D
        application; CachyOS's rebuild: Proton plus native Wayland and
        PipeWire drivers launched through the proton script directly, which
        upstream calls untested
```

The classification is a heuristic and the code says so. It is also
*evidence-based*: it reads the file rather than the filename, and when it
cannot tell it says `Unknown` instead of guessing.

## 2. The verbs, exactly

Read from the `proton` script, which dispatches six:

| Verb | What it does |
|---|---|
| `run` | Set up the drives and run. Does not wait for a previous session. |
| `waitforexitandrun` | `wineserver -w`, then the same as `run`. |
| `runinprefix` | Run inside an existing prefix without initialising it. |
| `destroyprefix` | Remove the files Proton tracks. Not an `rm -rf`. |
| `getcompatpath` | Unix path → Windows path. |
| `getnativepath` | Windows path → Unix path. |

**`getpwfmode` does not exist.** Several guides mention it; it appears on no
branch from 3.16 to 11.0. A launcher that calls it gets `Need a verb.` and
exit 1, which is easy to misreport as a crash.

## 3. The environment, and four bugs found by running it

Only two variables are required:

| Variable | If missing |
|---|---|
| `STEAM_COMPAT_DATA_PATH` | `exit 1`, "No compat data path?" |
| `STEAM_COMPAT_CLIENT_INSTALL_PATH` | an unguarded `KeyError` traceback |

`WINEPREFIX` is **overwritten** — the real prefix is `<DATA_PATH>/pfx`.

The rest of this section is what reading could not tell us. Every item below
was a real failure on the first run.

### 3.1 `protonfixes` reads digits out of the data path

```python
return re.findall(r'\d+', os.environ['STEAM_COMPAT_DATA_PATH'])[-1]
```

A path with no digits raises `IndexError` before anything starts. Steam
satisfies it with the app id; Tahoe has no app id, so it derives a stable one
from the target's name. Stable matters — the same program must keep its
prefix.

### 3.2 `protonfixes` requires `PWD` to contain `/steamapps`

```python
game_library = re.findall(r'.*/steamapps', os.environ['PWD'], re.IGNORECASE)[-1]
```

So the compat-data tree is laid out the way Steam lays it out —
`…/steamapps/compatdata/<id>` — and `PWD` is set to it. Every launcher that
runs Proton outside Steam builds this shape, for this reason.

### 3.3 Proton does not create its own data directory

`filelock` opens `<DATA_PATH>/pfx.lock` without creating the directory
first, so the launcher must. Tahoe's `CommandPlan` grew an `ensure_dirs`
field: the *plan* declares what it needs and the *supervisor* provides it,
which is what keeps planning a pure function.

### 3.4 The supervisor's environment whitelist was too strict

`PWD` is not a secret, but it is not in the whitelist either. For a layer
that reads it unguarded, the plan has to set it explicitly.

## 4. umu, the supported route

Upstream supports running Proton outside Steam **not at all**. The de-facto
supported route is **umu-launcher**, which is what Lutris and Bottles moved
to, and which is packaged for CachyOS as `cachyos/umu-launcher`.

Where `umu-run` exists, `tahoe-compat` prefers it and uses umu's own
variables — `PROTONPATH`, `GAMEID`, `PROTON_VERB` — rather than Proton's.
Where it does not, the direct route is used and **the summary says so**:
"which upstream calls untested".

## 5. Which Proton

On CachyOS the right one is `proton-cachyos` (`1:11.0.20261005-1` in the
`cachyos` repo): Proton plus native Wayland and PipeWire drivers. Discovery
prefers it, then Valve's own build, then GE-Proton.

| Build | What it adds | Note |
|---|---|---|
| Valve's | — | targeted at the Steam Linux Runtime |
| **CachyOS's** | native Wayland and PipeWire drivers | packaged for this distribution |
| GE-Proton | upstream experimental Wine + staging + codecs + protonfixes | a fork/rebuild |

Licensing: Proton is redistributable (BSD-3-Clause glue, LGPL-2.1 Wine, zlib
DXVK) **except `lsteamclient`**, which is under the Steamworks SDK license
and must not be shipped by a distribution. Tahoe ships none of it; it depends
on the user's installation, which is also how it stays current.

## 6. What was verified, and what was not

**Verified by running, on 2026-10-16:**

| | |
|---|---|
| Real PE built with clang + lld-link | `PE32+ executable for MS Windows, x86-64` |
| Classified from its import table | `["kernel32.dll", "d3d11.dll"]` → game |
| Routed to Proton-CachyOS | `Strategy::Proton`, `confidence=layer:proton` |
| **Executed** | **`exit_code: 0`**, 24.9 s |
| Prefix created by Proton | `…/steamapps/compatdata/game-472162599/pfx` |
| NTSync active | Proton reported `ntsync: up and running` |
| protonfixes ran | "All checks successful" |

**Not verified:** the program's own stdout did not appear in the captured
output of the Proton run, unlike the Wine run which printed `tahoe-pe-ok`.
Proton's `steam.exe` stub redirects the child's output, and confirming where
it goes is open work. The exit code is 0 and the prefix is real, so the
launch is proven; the output plumbing is not.

**Also not verified:** any real Windows game. Proton's compatibility is
Proton's to measure, and per-title results belong in a database, not in a
claim.
