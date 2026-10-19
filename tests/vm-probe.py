#!/usr/bin/env python3
# SPDX-License-Identifier: 0BSD
"""Drive a headless QEMU guest through its monitor socket: screenshot it and type at it.

    tests/vm-probe.py <iso> <outdir> --script-file <file>

Why this exists: `tests/vm-boot.sh` can only take pictures. When a boot fails
into an emergency shell, the shell itself is the best witness — it can say
which devices exist and what `blkid` sees — but a screendump cannot type. This
talks to a `-monitor unix:` socket, so it can send `sendkey` (real key events
into the guest console) and `screendump` (the answer, as a picture) in one
sequence.

The keyboard mapping is deliberately small: the keys needed to type a shell
command. It is not a general VNC client and should not become one.

Typing is slow on purpose. `sendkey` events go into a guest that may still be
booting, and a shell that misses the first characters of a word fails in a way
that looks like a different bug.
"""

import argparse
import socket
import subprocess
import sys
import time
from pathlib import Path

# Only the characters a shell command needs. Uppercase is a shifted letter.
PLAIN = {
    " ": "space", "-": "minus", "=": "equal", ".": "dot", ",": "comma",
    "/": "slash", ";": "semicolon", "'": "apostrophe", "\\": "backslash",
    "[": "bracket_left", "]": "bracket_right", "*": "shift-8", "_": "shift-minus",
    "$": "shift-4", "(": "shift-9", ")": "shift-0", ":": "shift-semicolon",
    "!": "shift-1", "|": "shift-backslash", "<": "shift-comma", ">": "shift-dot",
    "\n": "ret", "\t": "tab",
}


def keyname(ch: str) -> str:
    if ch.islower() or ch.isdigit():
        return ch
    if ch.isupper():
        return f"shift-{ch.lower()}"
    if ch in PLAIN:
        return PLAIN[ch]
    raise ValueError(f"no mapping for {ch!r}")


class Monitor:
    """A QEMU HMP monitor over a unix socket."""

    def __init__(self, path: Path, timeout: float = 30.0):
        deadline = time.time() + timeout
        last: Exception | None = None
        while time.time() < deadline:
            try:
                self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                self.sock.connect(str(path))
                self.sock.settimeout(5.0)
                time.sleep(0.3)
                self._drain()
                return
            except OSError as exc:  # not listening yet
                last = exc
                time.sleep(0.3)
        raise TimeoutError(f"monitor socket never appeared: {last}")

    def _drain(self) -> str:
        out = b""
        try:
            while True:
                block = self.sock.recv(65536)
                if not block:
                    break
                out += block
        except (TimeoutError, socket.timeout):
            pass
        return out.decode("utf-8", "replace")

    def cmd(self, line: str) -> str:
        self.sock.sendall(line.encode() + b"\n")
        time.sleep(0.4)
        return self._drain()


def build_qemu(args: argparse.Namespace, monitor_path: Path) -> list[str]:
    qemu: list[str] = []
    if args.qemu:
        qemu = args.qemu.split()
    else:
        qemu = ["flatpak", "run", "--filesystem=" + args.expose,
                "--command=qemu-system-x86_64", "org.virt_manager.virt-manager"]
    return qemu + [
        "-m", args.memory, "-smp", "4",
        "-enable-kvm", "-cpu", "host",
        "-machine", args.machine,
        "-device", "virtio-vga",
        "-display", "none",
        "-monitor", f"unix:{monitor_path},server,nowait",
        "-serial", "file:" + str(args.out / "serial.log"),
        "-drive", f"file={args.iso},media=cdrom,readonly=on,format=raw",
        "-boot", "d",
    ] + (["-drive", f"file={args.disk},if=virtio,format=raw,cache=unsafe"]
         if args.disk else [])


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("iso", type=Path)
    ap.add_argument("out", type=Path)
    ap.add_argument("--boot-wait", type=float, default=200.0,
                    help="seconds to let the guest boot before typing")
    ap.add_argument("--machine", default="q35")
    ap.add_argument("--memory", default="4096")
    ap.add_argument("--expose", default="/tmp/opencode")
    ap.add_argument("--qemu", default="", help="an explicit qemu command line prefix")
    ap.add_argument("--commands", nargs="*", default=[],
                    help="shell commands to type, one screenshot each")
    ap.add_argument("--keys", nargs="*", default=[],
                    help="raw QEMU key chords to send, e.g. meta_l-i for Super+I; "
                         "one screenshot after each. Chords are raw because the "
                         "keyboard map below only covers characters a shell command "
                         "needs, and a modifier combination is not a character.")
    ap.add_argument("--disk", default="",
                    help="attach this file as a virtio disk (created sparse if absent)")
    args = ap.parse_args()

    args.out.mkdir(parents=True, exist_ok=True)
    monitor_path = args.out / "monitor.sock"
    monitor_path.unlink(missing_ok=True)

    qemu = build_qemu(args, monitor_path)
    print("qemu:", " ".join(qemu[:6]), "...")
    proc = subprocess.Popen(qemu, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)

    try:
        mon = Monitor(monitor_path, timeout=60)
        print("monitor: connected")
        # QEMU's monitor prints a banner per connection; take a baseline frame
        # before touching the guest so "did we change anything" is answerable.
        mon.cmd(f"screendump {args.out}/probe-000-boot.ppm")
        print(f"waiting {args.boot_wait:.0f}s for the guest…")
        time.sleep(args.boot_wait)

        # Raw chords first: this is how the live session's SUPER+I reaches the
        # installer without a terminal to type in.
        for index, chord in enumerate(args.keys, start=1):
            mon.cmd(f"screendump {args.out}/probe-key-{index:03d}-before.ppm")
            mon.cmd(f"sendkey {chord}")
            print(f"sent chord: {chord}")
            time.sleep(6.0)
            mon.cmd(f"screendump {args.out}/probe-key-{index:03d}.ppm")

        for index, command in enumerate(args.commands, start=1):
            mon.cmd(f"screendump {args.out}/probe-{index:03d}-before.ppm")
            # The trailing Enter is not optional: without it the text sits in
            # the shell's line editor and the screenshot shows a prompt with a
            # command on it, which reads exactly like a command that ran and
            # printed nothing.
            # 0.08s, not 0.05: at 0.05 the guest console drops keys. The
            # symptom is a command that arrives with its spaces missing
            # ("echoSTART"), which looks like a shell bug and is not one.
            for ch in command + "\n":
                mon.cmd(f"sendkey {keyname(ch)}")
                time.sleep(0.08)
            time.sleep(1.5)
            mon.cmd(f"screendump {args.out}/probe-{index:03d}.ppm")
            print(f"sent: {command}")

        mon.cmd("screendump " + str(args.out / "probe-999-final.ppm"))
        mon.cmd("quit")
    except Exception as exc:  # noqa: BLE001 - report, do not hide
        print(f"probe failed: {exc}", file=sys.stderr)
        proc.terminate()
        return 1
    finally:
        try:
            proc.wait(timeout=20)
        except subprocess.TimeoutExpired:
            proc.terminate()
        err = proc.stderr.read().decode("utf-8", "replace") if proc.stderr else ""
        if err.strip():
            print("qemu stderr:", err.strip()[:500], file=sys.stderr)

    frames = sorted(args.out.glob("probe-*.ppm"))
    print(f"\n{len(frames)} frame(s); convert with tests/ppm-to-png.py")
    return 0


if __name__ == "__main__":
    sys.exit(main())
