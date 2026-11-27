#!/usr/bin/env bash
# Package an already-built target/release/megasip as a .deb, an .rpm and a .tgz.
#
#   packaging/linux-package.sh <source dir> <output dir>
#
# Run on the architecture being packaged (CI uses native x86_64 and arm64 runners), on
# Debian or Ubuntu with dpkg-dev and rpm installed. The source dir is the checkout the
# binary was built from: the version comes from its Cargo.toml and the desktop files from
# its data/. This script may come from a newer commit than the source, so that an older
# tag can be packaged again.
#
# Library dependencies are worked out from the binary (dpkg-shlibdeps for the .deb,
# rpmbuild's own ELF scan for the .rpm). The QML modules and the SVG plugin are loaded at
# run time, so those are listed by hand below.
set -euo pipefail

src="$(cd "$1" && pwd)"
mkdir -p "$2"
out="$(cd "$2" && pwd)"
here="$(cd "$(dirname "$0")" && pwd)"

version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$src/Cargo.toml" | head -n1)"
[ -n "$version" ] || { echo "no version in $src/Cargo.toml" >&2; exit 1; }
arch="$(uname -m)" # x86_64 or aarch64, as rpm and tarball names use them
deb_arch="$(dpkg --print-architecture)" # amd64 or arm64
bin="${CARGO_TARGET_DIR:-$src/target}/release/megasip"
[ -x "$bin" ] || { echo "$bin is missing; build it first" >&2; exit 1; }

summary="Voice-only SIP softphone, a Qt port of MicroSIP"
description="MegaSIP is a voice-only SIP softphone for Linux desktops: multiple accounts,
UDP/TCP/TLS, SRTP, hold, transfer, conference, call history, contacts and
sip:/tel: links. It uses the system's Qt 6, and Plasma's style and
notifications when they are installed."
url="https://github.com/tmcqueen/megasip"
maintainer="Tim McQueen <18832+tmcqueen@users.noreply.github.com>"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# The installed file tree, shared by the .deb and the .rpm.
root="$work/root"
install -Dm755 "$bin" "$root/usr/bin/megasip"
install -Dm644 "$src/data/org.megasip.MegaSIP.desktop" \
    "$root/usr/share/applications/org.megasip.MegaSIP.desktop"
install -Dm644 "$src/data/org.megasip.MegaSIP.svg" \
    "$root/usr/share/icons/hicolor/scalable/apps/org.megasip.MegaSIP.svg"
install -Dm644 "$src/data/megasip.notifyrc" "$root/usr/share/knotifications6/megasip.notifyrc"
install -Dm644 "$src/LICENSE" "$root/usr/share/doc/megasip/LICENSE"
install -Dm644 "$src/crates/megasip-qt/icons/README.md" "$root/usr/share/doc/megasip/ICONS-LICENSE.md"

# --- .deb -------------------------------------------------------------------------------
deb_runtime="qml6-module-qtquick, qml6-module-qtquick-controls, qml6-module-qtquick-layouts,
 qml6-module-qtquick-dialogs, qml6-module-qtquick-effects, qml6-module-qtqml-models,
 qml6-module-qtquick-templates, qml6-module-qtquick-window, qml6-module-qt-labs-platform,
 qt6-svg-plugins"
deb_recommends="qml6-module-org-kde-qqc2desktopstyle, qml6-module-org-kde-notifications"

debroot="$work/deb"
cp -a "$root" "$debroot"
mkdir -p "$debroot/DEBIAN" "$work/shlibs/debian"
# dpkg-shlibdeps wants to be run from a source package; give it an empty one.
printf 'Source: megasip\n\nPackage: megasip\nArchitecture: any\n' >"$work/shlibs/debian/control"
shlibs="$(cd "$work/shlibs" && dpkg-shlibdeps -O "$debroot/usr/bin/megasip" | sed -n 's/^shlibs:Depends=//p')"
# The QML that cxx-qt compiles ahead of time (qmlcachegen) calls a few Qt-private
# functions, so dpkg-shlibdeps pins the package to the exact Qt it was built with
# (qt6-*-private-abi (= 6.8.2)), and it couldn't install on a newer Ubuntu. Drop the pin:
# the release workflow installs the package on newer Qt and runs every screen, so a Qt
# that breaks it shows up there.
shlibs="$(echo "$shlibs" | sed -E 's/, *qt6-[a-z]+-private-abi \([^)]*\)//g; s/^qt6-[a-z]+-private-abi \([^)]*\), *//')"
installed_kb="$(du -sk "$root" | cut -f1)"
cat >"$debroot/DEBIAN/control" <<EOF
Package: megasip
Version: $version
Architecture: $deb_arch
Maintainer: $maintainer
Installed-Size: $installed_kb
Depends: $shlibs, $(echo $deb_runtime)
Recommends: $deb_recommends
Section: comm
Priority: optional
Homepage: $url
Description: $summary
$(echo "$description" | sed 's/^/ /')
EOF
deb="$out/megasip_${version}_${deb_arch}.deb"
dpkg-deb --root-owner-group -Zxz --build "$debroot" "$deb"

# --- .rpm -------------------------------------------------------------------------------
# Fedora package names. rpmbuild adds the libQt6*.so.6 and libc requirements itself.
rpm_runtime="qt6-qtdeclarative qt6-qtsvg"
rpm_recommends="kf6-qqc2-desktop-style kf6-knotifications"
rpmtop="$work/rpm"
mkdir -p "$rpmtop"/{BUILD,RPMS,SPECS,BUILDROOT}
# The same Qt-private functions as above: rpmbuild would require
# libQt6Qml.so.6(Qt_6_PRIVATE_API), which Fedora's Qt doesn't provide under that name.
# Drop it the same way.
cat >"$rpmtop/SPECS/megasip.spec" <<EOF
%global __requires_exclude ^lib.*\\(Qt_6_PRIVATE_API\\)
Name:           megasip
Version:        $version
Release:        1
Summary:        $summary
License:        GPL-2.0-or-later
URL:            $url
Requires:       $rpm_runtime
Recommends:     $(echo $rpm_recommends | sed 's/ /, /g')

%description
$description

%install
cp -a "$root/." %{buildroot}/

%files
/usr/bin/megasip
/usr/share/applications/org.megasip.MegaSIP.desktop
/usr/share/icons/hicolor/scalable/apps/org.megasip.MegaSIP.svg
/usr/share/knotifications6/megasip.notifyrc
%license /usr/share/doc/megasip/LICENSE
%doc /usr/share/doc/megasip/ICONS-LICENSE.md
EOF
rpmbuild -bb --quiet \
    --define "_topdir $rpmtop" \
    --define "_build_id_links none" \
    --define "debug_package %{nil}" \
    --define "__os_install_post %{nil}" \
    "$rpmtop/SPECS/megasip.spec"
rpm="$out/megasip-${version}-1.${arch}.rpm"
mv "$rpmtop/RPMS/$arch/"*.rpm "$rpm"

# --- .tgz -------------------------------------------------------------------------------
name="megasip-${version}-linux-${arch}"
tdir="$work/$name"
mkdir -p "$tdir"
install -m755 "$bin" "$tdir/megasip"
install -m755 "$here/install.sh" "$tdir/install.sh"
cp "$src/data/org.megasip.MegaSIP.desktop" "$src/data/org.megasip.MegaSIP.svg" \
    "$src/data/megasip.notifyrc" "$src/LICENSE" "$tdir/"
cp "$src/crates/megasip-qt/icons/README.md" "$tdir/ICONS-LICENSE.md"
cp "$here/TARBALL-README.md" "$tdir/README.md"
tar -C "$work" --owner=0 --group=0 -czf "$out/$name.tgz" "$name"

echo "Packaged MegaSIP $version for $arch:"
ls -l "$out"
echo "--- deb Depends ---"
dpkg-deb -f "$deb" Depends
echo "--- rpm Requires ---"
rpm -qp --requires "$rpm"
