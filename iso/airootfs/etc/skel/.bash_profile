# SPDX-License-Identifier: GPL-3.0-or-later
# Project Tahoe live — start the desktop session automatically on tty1.
#
# Maintenance: switch to tty2..tty6 (Ctrl+Alt+F2 …), log in as root (no
# password on the live image) or as `tahoe`. This block only ever fires on
# tty1, so those consoles always give you an ordinary shell.
if [ -x /usr/bin/Hyprland ] && [ "$(tty 2>/dev/null)" = "/dev/tty1" ] && [ -z "${TAHOE_SESSION:-}" ]; then
    export TAHOE_SESSION=1
    exec /usr/bin/Hyprland
fi
