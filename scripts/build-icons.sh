#!/usr/bin/env bash
# Regenerates every app and tray icon from the SVG sources in assets/.
#   assets/icon.svg        full-detail icon (sizes above 44px)
#   assets/icon-small.svg  heavier strokes for 16-44px, so "50" stays legible
#   assets/tray.svg        single-colour glyph for the macOS menu-bar template
# Needs macOS (iconutil), rsvg-convert (librsvg) and ImageMagick: brew install librsvg imagemagick
set -euo pipefail
cd "$(dirname "$0")/.."

icons=src-tauri/icons
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

render() { rsvg-convert -w "$2" -h "$2" "$1" -o "$3"; }

# 1. Base set from the full icon, then drop the mobile outputs (desktop-only app).
render assets/icon.svg 1024 assets/icon.png
pnpm tauri icon assets/icon.png > /dev/null 2>&1
rm -rf "$icons/android" "$icons/ios"

# 2. Small sizes come from the heavy-stroke variant.
render assets/icon-small.svg 32 "$icons/32x32.png"
render assets/icon-small.svg 30 "$icons/Square30x30Logo.png"
render assets/icon-small.svg 44 "$icons/Square44x44Logo.png"

# 3. Windows .ico: small variant up to 32px, full icon above.
for s in 16 24 32; do render assets/icon-small.svg $s "$tmp/ico-$s.png"; done
for s in 48 64 256; do render assets/icon.svg $s "$tmp/ico-$s.png"; done
magick "$tmp"/ico-{16,24,32,48,64,256}.png "$icons/icon.ico"

# 4. macOS .icns: same split.
set_dir="$tmp/icon.iconset"
mkdir "$set_dir"
render assets/icon-small.svg 16 "$set_dir/icon_16x16.png"
render assets/icon-small.svg 32 "$set_dir/icon_16x16@2x.png"
render assets/icon-small.svg 32 "$set_dir/icon_32x32.png"
render assets/icon.svg 64 "$set_dir/icon_32x32@2x.png"
for s in 128 256 512; do
  render assets/icon.svg $s "$set_dir/icon_${s}x${s}.png"
  render assets/icon.svg $((s * 2)) "$set_dir/icon_${s}x${s}@2x.png"
done
iconutil -c icns "$set_dir" -o "$icons/icon.icns"

# 5. Tray: monochrome template for macOS (18pt @2x), coloured small icon for Windows/Linux.
render assets/tray.svg 36 "$icons/tray-template.png"
render assets/icon-small.svg 64 "$icons/tray-color.png"

# 6. The popover's header logo (26px), at 3x. An SVG with filters renders soft in WebKit.
render assets/icon-small.svg 78 src/lib/assets/logo.png

echo "icons regenerated in $icons"
