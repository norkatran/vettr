#!/usr/bin/env bash
# Renders PNGs, the app icon set and favicon.ico from the SVGs. Run build_logo.py first.
# Needs ImageMagick (`magick`, with librsvg).  Usage: ./branding/build_assets.sh
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p png icons
tmp=$(mktemp -d)

render() { magick -background none -density 1200 "$1" -resize "$2" "$3"; }

for n in symbol horizontal stacked wordmark; do
  w=2400; [ "$n" = symbol ] && w=1024
  for v in "" "-on-dark"; do render "masters/vettr-$n$v.svg" "${w}x" "png/vettr-$n$v.png"; done
  for v in black white; do render "exports/vettr-$n-$v.svg" "${w}x" "png/vettr-$n-$v.png"; done
done

for sz in 1024 512 256 128 64 32 16; do
  render masters/vettr-app-icon.svg "${sz}x${sz}" "icons/vettr-app-icon-$sz.png"
done
render masters/vettr-app-icon-square.svg 180x180 icons/apple-touch-icon.png
render masters/vettr-app-icon-square.svg 512x512 icons/maskable-512.png
for sz in 16 32 48; do render masters/vettr-favicon.svg "${sz}x${sz}" "$tmp/fav-$sz.png"; done
magick "$tmp/fav-16.png" "$tmp/fav-32.png" "$tmp/fav-48.png" icons/favicon.ico
cp masters/vettr-favicon.svg icons/favicon.svg
cp masters/vettr-app-icon.svg icons/vettr-app-icon.svg
echo "assets written"
