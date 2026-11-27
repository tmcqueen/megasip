# MegaSIP

A voice-only SIP softphone for Linux desktops. This tarball holds a ready-built `megasip`
binary. Where you can, use the `.deb` or `.rpm` from the same release instead: they pull
in Qt for you and your package manager can update or remove them.

## Install

MegaSIP uses the system's Qt 6.5 or newer (Debian 13, Ubuntu 24.10+, Fedora 39+). Install
its runtime pieces first:

```sh
# Debian, Ubuntu
sudo apt install qml6-module-qtquick-controls qml6-module-qtquick-layouts \
    qml6-module-qtquick-dialogs qml6-module-qtquick-effects qml6-module-qtqml-models \
    qml6-module-qt-labs-platform qt6-svg-plugins
# Fedora
sudo dnf install qt6-qtdeclarative qt6-qtsvg
```

On Plasma, also install its style and notifications (with Answer / Decline):
`qml6-module-org-kde-qqc2desktopstyle qml6-module-org-kde-notifications` (Debian, Ubuntu)
or `kf6-qqc2-desktop-style kf6-knotifications` (Fedora).

Then:

```sh
./install.sh            # into ~/.local, for you only
sudo ./install.sh       # into /usr/local, for everyone
./install.sh --uninstall
```

`install.sh` puts the binary, desktop entry, icon and notification config in place, and
registers MegaSIP for `sip:`/`tel:` links where no other app handles them. Set `PREFIX` to
install somewhere else.

## License

GPL-2.0-or-later (`LICENSE`). The bundled Breeze icons are LGPL-3.0-or-later
(`ICONS-LICENSE.md`).
