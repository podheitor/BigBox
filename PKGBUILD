# Maintainer: Heitor Faria <heitorfaria@gmail.com>
pkgname=bigbox
pkgver=0.1.0
pkgrel=1
pkgdesc="All your messaging apps in one blazing-fast window. Lightweight Rambox alternative built with Tauri v2 + Rust."
arch=('x86_64')
url="https://github.com/podheitor/BigBox"
license=('GPL-3.0-or-later')
depends=('webkit2gtk-4.1' 'gtk3' 'libayatana-appindicator'
         'gst-plugins-base' 'gst-plugins-good' 'gst-plugins-bad' 'gst-libav' 'xdg-utils')
makedepends=('rust' 'cargo')
provides=('bigbox')
conflicts=('bigbox')
source=("${pkgname}-${pkgver}.tar.gz::https://github.com/podheitor/BigBox/archive/refs/tags/v${pkgver}.tar.gz")
sha256sums=('SKIP')

build() {
    cd "BigBox-${pkgver}"
    export CARGO_HOME="${srcdir}/cargo-home"
    cargo install tauri-cli --version "^2" --root "${srcdir}/tools"
    PATH="${srcdir}/tools/bin:${PATH}" cargo tauri build --bundles none
}

package() {
    cd "BigBox-${pkgver}"
    install -Dm755 "src-tauri/target/release/bigbox" "${pkgdir}/usr/bin/bigbox"

    # Desktop entry
    install -Dm644 /dev/stdin "${pkgdir}/usr/share/applications/bigbox.desktop" << 'DESKTOP'
[Desktop Entry]
Name=BigBox
Comment=All your messaging apps in one blazing-fast window
Exec=bigbox
Icon=bigbox
Type=Application
Categories=Network;InstantMessaging;
Keywords=whatsapp;telegram;gmail;slack;discord;messaging;
StartupWMClass=BigBox
DESKTOP

    # SVG icon
    if [[ -f "frontend/bigbox.svg" ]]; then
        install -Dm644 "frontend/bigbox.svg" \
            "${pkgdir}/usr/share/icons/hicolor/scalable/apps/bigbox.svg"
    fi
}
