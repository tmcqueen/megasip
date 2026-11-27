# Bundled icons

`megasip-light` and `megasip-dark` hold the icons MegaSIP's UI uses, copied from KDE's
[Breeze icons](https://invent.kde.org/frameworks/breeze-icons) (`breeze` and `breeze-dark`,
version 6.24). They are licensed under the LGPL-3.0-or-later.

They are compiled into the app (`:/icons/...`). Where the desktop has no icon theme, as on
Windows, the matching one is the icon theme; elsewhere it fills in names the desktop's theme
lacks. When the QML starts using a new icon name, copy its 16, 22, 32 and 48 px SVGs from
both Breeze themes into the same folders here.
