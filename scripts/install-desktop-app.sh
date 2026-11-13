#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
appimage_dir="$repo_root/target/release/bundle/appimage"
appimage="$(find "$appimage_dir" -maxdepth 1 -type f -name '*.AppImage' -print -quit)"
if [[ -z "$appimage" ]]; then
  printf 'No AppImage found in %s\n' "$appimage_dir" >&2
  exit 1
fi

install -Dm755 "$appimage" "$HOME/.local/bin/cigmail.AppImage"
for size in 32x32 128x128; do
  install -Dm644 "$repo_root/src-tauri/icons/$size.png" "$HOME/.local/share/icons/hicolor/$size/apps/cigmail.png"
done
install -Dm644 "$repo_root/src-tauri/icons/128x128@2x.png" "$HOME/.local/share/icons/hicolor/256x256/apps/cigmail.png"
install -Dm644 "$repo_root/src-tauri/icons/icon.png" "$HOME/.local/share/icons/hicolor/512x512/apps/cigmail.png"
install -Dm644 "$repo_root/src-tauri/icons/icon.svg" "$HOME/.local/share/icons/hicolor/scalable/apps/cigmail.svg"
install -d "$HOME/.local/share/applications"
cat > "$HOME/.local/share/applications/cigmail.desktop" <<EOF
[Desktop Entry]
Name=CigMail
Comment=Fast, keyboard-first unified email
Exec=$HOME/.local/bin/cigmail.AppImage
Icon=cigmail
Terminal=false
Type=Application
Categories=Network;Email;Office;
StartupNotify=true
EOF
chmod 644 "$HOME/.local/share/applications/cigmail.desktop"
printf 'Installed %s to %s\n' "$(basename "$appimage")" "$HOME/.local/bin/cigmail.AppImage"
