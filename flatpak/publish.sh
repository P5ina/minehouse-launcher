#!/usr/bin/env bash
# Builds the Flatpak from a release .deb and writes a static site with a signed
# Flatpak repository:
#   <out>/repo/                          OSTree repo (branch "stable")
#   <out>/pro.minehouse.launcher.flatpakref   one-click install
#   <out>/minehouse.flatpakrepo          `flatpak remote-add`
#   <out>/MineHouse_<version>_x86_64.flatpak  single-file bundle for the release
#   <out>/index.html
#
# Usage: flatpak/publish.sh <deb> <version> <gpg-key-id> <site-url> <out-dir>
# The GPG key must be in the default keyring (CI imports FLATPAK_GPG_PRIVATE_KEY).
set -euo pipefail

deb=$(realpath "$1")
version=$2
key=$3
site=${4%/}
out=$(realpath -m "$5")

here=$(dirname "$(realpath "$0")")
app=pro.minehouse.launcher
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

cp "$here/$app.yml" "$here/$app.metainfo.xml" "$work/"
cp "$deb" "$work/minehouse-launcher.deb"

mkdir -p "$out"
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo

flatpak-builder --user --force-clean --disable-rofiles-fuse \
  --install-deps-from=flathub \
  --default-branch=stable \
  --gpg-sign="$key" \
  --repo="$out/repo" \
  "$work/build" "$work/$app.yml"

flatpak build-update-repo \
  --title="MineHouse" \
  --default-branch=stable \
  --gpg-sign="$key" \
  --generate-static-deltas \
  --prune \
  "$out/repo"

flatpak build-bundle \
  --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo \
  --gpg-sign="$key" \
  "$out/repo" "$out/MineHouse_${version}_x86_64.flatpak" "$app" stable

gpgkey=$(gpg --export "$key" | base64 -w0)
cp "$work/build/files/share/icons/hicolor/128x128/apps/$app.png" "$out/icon.png"

cat > "$out/minehouse.flatpakrepo" <<EOF
[Flatpak Repo]
Title=MineHouse
Url=$site/repo/
Homepage=https://www.minehouse.pro
Comment=MineHouse launcher
Icon=$site/icon.png
GPGKey=$gpgkey
EOF

cat > "$out/$app.flatpakref" <<EOF
[Flatpak Ref]
Name=$app
Branch=stable
Title=MineHouse
Url=$site/repo/
SuggestRemoteName=minehouse
Homepage=https://www.minehouse.pro
Icon=$site/icon.png
RuntimeRepo=https://dl.flathub.org/repo/flathub.flatpakrepo
IsRuntime=false
GPGKey=$gpgkey
EOF

cat > "$out/index.html" <<EOF
<!doctype html>
<html lang="ru">
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>MineHouse для Linux</title>
<style>
  body { font: 16px/1.5 system-ui, sans-serif; max-width: 42rem; margin: 3rem auto; padding: 0 1rem; background: #09090b; color: #fafafa; }
  code, pre { font-family: ui-monospace, monospace; background: #18181b; border-radius: 6px; }
  pre { padding: .75rem 1rem; overflow-x: auto; }
  a { color: #e4e4e7; }
</style>
<h1>MineHouse для Linux (Flatpak)</h1>
<p>Версия $version. Установка:</p>
<pre>flatpak install --user $site/$app.flatpakref</pre>
<p>Обновление вместе с остальными приложениями:</p>
<pre>flatpak update</pre>
<p>Или скачайте файл <a href="MineHouse_${version}_x86_64.flatpak">MineHouse_${version}_x86_64.flatpak</a>.
Лаунчер и сайт сервера: <a href="https://www.minehouse.pro">minehouse.pro</a>.</p>
</html>
EOF
touch "$out/.nojekyll"
echo "Published $app $version to $out"
