#!/usr/bin/env bash
# Install MegaSIP from this tarball.
#
#   ./install.sh               installs for you, into ~/.local (no root needed)
#   sudo ./install.sh          installs for everyone, into /usr/local
#   PREFIX=/opt/megasip ./install.sh
#   ./install.sh --uninstall   removes what an install with the same PREFIX put there
#
# MegaSIP uses the system's Qt 6 (Debian 13 or Ubuntu 26.04 and newer). The .deb pulls it in;
# with this tarball, install it yourself first (see README.md).
set -euo pipefail
cd "$(dirname "$0")"

if [ -z "${PREFIX:-}" ]; then
    if [ "$(id -u)" -eq 0 ]; then PREFIX=/usr/local; else PREFIX="$HOME/.local"; fi
fi

files=(
    "bin/megasip"
    "share/applications/org.megasip.MegaSIP.desktop"
    "share/icons/hicolor/scalable/apps/org.megasip.MegaSIP.svg"
    "share/knotifications6/megasip.notifyrc"
)

refresh() {
    update-desktop-database "$PREFIX/share/applications" 2>/dev/null || true
    gtk-update-icon-cache -q "$PREFIX/share/icons/hicolor" 2>/dev/null || true
    kbuildsycoca6 >/dev/null 2>&1 || true
}

if [ "${1:-}" = "--uninstall" ]; then
    for f in "${files[@]}"; do rm -f "$PREFIX/$f"; done
    refresh
    echo "Removed MegaSIP from $PREFIX. Your accounts and history stay in ~/.local/share/megasip."
    exit 0
elif [ -n "${1:-}" ]; then
    sed -n '2,8s/^# \{0,1\}//p' "$(basename "$0")"
    exit 2
fi

install -Dm755 megasip "$PREFIX/bin/megasip"
install -Dm644 org.megasip.MegaSIP.desktop "$PREFIX/share/applications/org.megasip.MegaSIP.desktop"
install -Dm644 org.megasip.MegaSIP.svg "$PREFIX/share/icons/hicolor/scalable/apps/org.megasip.MegaSIP.svg"
install -Dm644 megasip.notifyrc "$PREFIX/share/knotifications6/megasip.notifyrc"
refresh

# Make MegaSIP the handler for sip:/tel: links only where nothing else is set, and only
# for the user running this (System Settings > Default Applications can change it).
if [ "$(id -u)" -ne 0 ]; then
    for scheme in sip sips tel callto; do
        if [ -z "$(xdg-mime query default "x-scheme-handler/$scheme" 2>/dev/null)" ]; then
            xdg-mime default org.megasip.MegaSIP.desktop "x-scheme-handler/$scheme" 2>/dev/null || true
        fi
    done
fi

echo "Installed to $PREFIX/bin/megasip"
case ":$PATH:" in *":$PREFIX/bin:"*) ;; *) echo "Note: add $PREFIX/bin to your PATH" ;; esac

# What the dynamic linker can't satisfy: a missing library, or a C library older than the
# one MegaSIP was built against. QML modules are only looked up at run time, so a missing
# one shows when MegaSIP starts; README.md lists the packages.
problems="$(ldd "$PREFIX/bin/megasip" 2>&1 | sed -n \
    -e 's/^[[:space:]]*\([^[:space:]]*\) => not found.*/\1/p' \
    -e "s/.*version \`\([^']*\)' not found.*/\1 (this system's C library is too old)/p" | sort -u)" || true
if [ -n "$problems" ]; then
    echo
    echo "MegaSIP won't start yet. Missing:"
    echo "$problems" | sed 's/^/  /'
    echo "See README.md for the packages to install."
fi
