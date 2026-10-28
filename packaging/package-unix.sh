#!/usr/bin/env bash
# Package already-built binaries; no compilation or downloads happen here.
set -euo pipefail

target=${1:-}
case "$target" in
  aarch64-apple-darwin|x86_64-unknown-linux-gnu) ;;
  *) echo 'Usage: package-unix.sh aarch64-apple-darwin|x86_64-unknown-linux-gnu' >&2; exit 1 ;;
esac
version=$(bash packaging/version.sh)
binary_dir=${ICATALOG_RELEASE_BINARY_DIR:-target/$target/release}
output_dir=${ICATALOG_RELEASE_OUTPUT_DIR:-target/release-assets}
for binary in icatalog icatalog-ui; do
  if [[ ! -x "$binary_dir/$binary" ]]; then
    echo "Missing executable $binary_dir/$binary; run task release:build -- --target $target first." >&2
    exit 1
  fi
done
mkdir -p "$output_dir" target
stage=$(mktemp -d target/release-stage.XXXXXX)
trap 'rm -rf "$stage"' EXIT

if [[ "$target" == aarch64-apple-darwin ]]; then
  bundle="$stage/iCatalog.app/Contents"
  mkdir -p "$bundle/MacOS"
  install -m 755 "$binary_dir/icatalog-ui" "$bundle/MacOS/icatalog-ui"
  install -m 755 "$binary_dir/icatalog" "$stage/icatalog"
  cat > "$bundle/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>iCatalog</string>
  <key>CFBundleDisplayName</key><string>iCatalog</string>
  <key>CFBundleIdentifier</key><string>org.icatalog.desktop</string>
  <key>CFBundleExecutable</key><string>icatalog-ui</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${version%%-*}</string>
  <key>CFBundleVersion</key><string>${version%%-*}</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
EOF
  plutil -lint "$bundle/Info.plist"
  # Ad-hoc signing supports Apple Silicon execution; this is not Developer ID signing.
  codesign --force --sign - "$stage/iCatalog.app"
  codesign --verify --strict "$stage/iCatalog.app"
  # Avoid AppleDouble sidecars appearing as extra sealed resources in other extractors.
  COPYFILE_DISABLE=1 tar -czf "$output_dir/icatalog-$version-macos-arm64.tar.gz" -C "$stage" iCatalog.app icatalog
else
  install -d "$stage/usr/bin" "$stage/usr/share/applications"
  install -m 755 "$binary_dir/icatalog" "$binary_dir/icatalog-ui" "$stage/usr/bin/"
  install -m 644 packaging/icatalog.desktop "$stage/usr/share/applications/"
  # Debian/RPM use ~ for prereleases; Arch forbids '-' in pkgver.
  package_version=${version/-/~}
  common=(-s dir -n icatalog --iteration 1
    --license MIT --description 'iCatalog media catalogue and command-line interface'
    --maintainer 'iCatalog contributors' -C "$stage")
  fpm "${common[@]}" -v "$package_version" -t deb -a amd64 --deb-no-default-config-files \
    --depends 'libc6 >= 2.35' --depends libgcc-s1 --depends libstdc++6 \
    --depends libxkbcommon0 --depends libwayland-client0 --depends libx11-6 \
    --depends libegl1 --depends libvulkan1 --depends xdg-desktop-portal \
    -p "$output_dir/icatalog-$version-linux-amd64.deb" usr
  fpm "${common[@]}" -v "$package_version" -t rpm -a x86_64 --rpm-auto-add-directories \
    --depends 'glibc >= 2.35' --depends libgcc --depends libstdc++ \
    --depends libxkbcommon --depends libwayland-client --depends libX11 \
    --depends libglvnd-egl --depends vulkan-loader --depends xdg-desktop-portal \
    -p "$output_dir/icatalog-$version-linux-x86_64.rpm" usr
  arch_package="$output_dir/icatalog-$version-linux-x86_64.pkg.tar.zst"
  fpm "${common[@]}" -v "${version//-/_}" -t pacman -a x86_64 --pacman-compression zstd \
    --depends 'glibc>=2.35' --depends gcc-libs --depends libxkbcommon \
    --depends wayland --depends libx11 --depends libglvnd \
    --depends vulkan-icd-loader --depends xdg-desktop-portal \
    -p "$arch_package" usr
  bash packaging/check-arch-package.sh "$arch_package"
fi
