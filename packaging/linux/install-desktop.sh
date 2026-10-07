#!/bin/sh
# Installs the vettr desktop entry and icons for the current user so Linux
# window managers (notably Wayland, which ignores window-set icons) show the logo.
set -e
root=$(cd "$(dirname "$0")/../.." && pwd)
data="${XDG_DATA_HOME:-$HOME/.local/share}"
for s in 16 32 64 128 256 512; do
  d="$data/icons/hicolor/${s}x${s}/apps"
  mkdir -p "$d"
  cp "$root/branding/icons/vettr-app-icon-$s.png" "$d/vettr.png"
done
mkdir -p "$data/icons/hicolor/scalable/apps" "$data/applications"
cp "$root/branding/icons/vettr-app-icon.svg" "$data/icons/hicolor/scalable/apps/vettr.svg"
# Point Exec at the built binary when it is not on PATH.
bin="$root/target/release/vettr"
sed "s|^Exec=.*|Exec=$bin|" "$root/packaging/linux/vettr.desktop" > "$data/applications/vettr.desktop"
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -qtf "$data/icons/hicolor" || true
command -v update-desktop-database >/dev/null && update-desktop-database "$data/applications" || true
echo "Installed vettr desktop entry to $data"
