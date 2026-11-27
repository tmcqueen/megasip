# MegaSIP

A voice-only SIP softphone for Linux desktops. This tarball holds a ready-built `megasip`
binary, built on Debian 13 for Debian- and Ubuntu-family systems. Where you can, use the
`.deb` from the same release instead: it pulls in Qt for you and your package manager can
update or remove it. On Fedora, use the `.rpm`; this binary doesn't run against Fedora's Qt.

## Install

MegaSIP uses the system's Qt (Debian 13 or newer, Ubuntu 26.04 or newer). Install its
runtime pieces first:

```sh
sudo apt install qml6-module-qtquick-controls qml6-module-qtquick-layouts \
    qml6-module-qtquick-dialogs qml6-module-qtquick-effects qml6-module-qtqml-models \
    qml6-module-qt-labs-platform qt6-svg-plugins
```

On Plasma, also install its style and notifications (with Answer / Decline):
`qml6-module-org-kde-qqc2desktopstyle qml6-module-org-kde-notifications`.

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
