#!/usr/bin/env bash
# Build MegaSIP and install it for the current user (~/.local). No root needed.
set -euo pipefail
cd "$(dirname "$0")"

cargo build --release

PREFIX="${PREFIX:-$HOME/.local}"
install -Dm755 target/release/megasip "$PREFIX/bin/megasip"
install -Dm644 data/org.megasip.MegaSIP.desktop "$PREFIX/share/applications/org.megasip.MegaSIP.desktop"
install -Dm644 data/org.megasip.MegaSIP.svg "$PREFIX/share/icons/hicolor/scalable/apps/org.megasip.MegaSIP.svg"
install -Dm644 data/megasip.notifyrc "$PREFIX/share/knotifications6/megasip.notifyrc"

# Let Plasma find the new entry. MegaSIP becomes the handler for sip:/tel: links only
# where nothing else is set (System Settings > Default Applications can change it).
update-desktop-database "$PREFIX/share/applications" 2>/dev/null || true
gtk-update-icon-cache -q "$PREFIX/share/icons/hicolor" 2>/dev/null || true
for scheme in sip sips tel callto; do
    if [ -z "$(xdg-mime query default "x-scheme-handler/$scheme" 2>/dev/null)" ]; then
        xdg-mime default org.megasip.MegaSIP.desktop "x-scheme-handler/$scheme" 2>/dev/null || true
    fi
done
kbuildsycoca6 >/dev/null 2>&1 || true

echo "Installed to $PREFIX/bin/megasip"
case ":$PATH:" in *":$PREFIX/bin:"*) ;; *) echo "Note: add $PREFIX/bin to your PATH";; esac
