# Maintainer: EchoInput Team <team@echoinput.dev>
pkgname=echoinput
pkgver=0.1.0
pkgrel=1
pkgdesc="Privacy-first keyboard visualization overlay for Wayland, X11, Windows, and macOS"
arch=('x86_64')
url="https://github.com/SuperSection/echoinput"
license=('MIT' 'Apache-2.0')
depends=('gcc-libs' 'libxkbcommon' 'wayland' 'cairo' 'glib2')
makedepends=('rust' 'cargo')
source=("$pkgname-$pkgver.tar.gz::https://github.com/SuperSection/echoinput/archive/refs/tags/v$pkgver.tar.gz")
sha256sums=('SKIP')

prepare() {
    cd "$pkgname-$pkgver"
    cargo fetch --locked --target "$CARCH-unknown-linux-gnu"
}

build() {
    cd "$pkgname-$pkgver"
    export CARGO_TARGET_DIR=target
    cargo build --frozen --release --all-features
}

check() {
    cd "$pkgname-$pkgver"
    cargo test --frozen --all-features
}

package() {
    cd "$pkgname-$pkgver"
    install -Dm755 "target/release/echoinput" "$pkgdir/usr/bin/echoinput"

    # Desktop entry
    install -Dm644 /dev/stdin "$pkgdir/usr/share/applications/echoinput.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=EchoInput
Comment=Keyboard visualization overlay
Exec=echoinput
Icon=echoinput
Categories=Utility;
StartupNotify=false
EOF

    # Icon (using a placeholder - replace with actual icon)
    # install -Dm644 "assets/icon.png" "$pkgdir/usr/share/icons/hicolor/256x256/apps/echoinput.png"
}
