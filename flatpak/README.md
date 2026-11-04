# Flatpak / Flathub

App ID `pro.minehouse.launcher` (совпадает с `identifier` в `tauri.conf.json`).

| Файл | Что |
| --- | --- |
| `pro.minehouse.launcher.yml` | Манифест. Тот же файл лежит в `github.com/flathub/pro.minehouse.launcher` |
| `flathub.json` | Только x86_64: Linux-релиз собирается только под него |
| `pro.minehouse.launcher.metainfo.xml` | AppStream: описание, скриншоты, история версий. Попадает в `.deb` (`bundle.linux.deb.files`), оттуда во Flatpak |
| `../docs/screenshots/` | Скриншоты для Flathub, ссылки в metainfo на тег версии |

Манифест перепаковывает `.deb` из GitHub-релиза (так советует Tauri): бинарник,
desktop-файл (переименовывается в `pro.minehouse.launcher.desktop`), иконки и metainfo.
Рантайм `org.gnome.Platform` уже содержит WebKitGTK 4.1.

Внутри Flatpak встроенное автообновление выключено (`in_flatpak()` в
`src-tauri/src/main.rs`: плагин updater не подключается, баннер не показывается),
обновляет Flathub.

## Новая версия

1. Добавить `<release version="X.Y.Z" date="…">` в metainfo **до** релиза.
2. Выпустить релиз как обычно (Actions → Release).
3. Бот Flathub (`x-checker-data` в манифесте) сам откроет PR в
   `flathub/pro.minehouse.launcher` с новым URL и sha256 `.deb`, его достаточно смёржить.

## Собрать локально

```sh
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak install --user flathub org.flatpak.Builder
flatpak run org.flatpak.Builder --user --force-clean --install-deps-from=flathub \
  --repo=repo --install build flatpak/pro.minehouse.launcher.yml
flatpak run pro.minehouse.launcher

# линтер Flathub
flatpak run --command=flatpak-builder-lint org.flatpak.Builder manifest flatpak/pro.minehouse.launcher.yml
flatpak run --command=flatpak-builder-lint org.flatpak.Builder repo repo
```

Чтобы собрать из своей ветки, замените в манифесте `url:` + `sha256:` на
`path: ../target/release/bundle/deb/MineHouse_<версия>_amd64.deb` после
`pnpm tauri build --bundles deb`.

## Права

- `--socket=x11` вместе с `--socket=wayland`: окно лаунчера работает на Wayland,
  а сам Minecraft (LWJGL/GLFW) только на X11/XWayland. Линтер выдаёт на это
  предупреждение, не ошибку.
- `--device=dri` — OpenGL для игры, `--socket=pulseaudio` — звук и микрофон для голосового чата.
- `--talk-name=org.freedesktop.secrets` — refresh-токен Microsoft в хранилище паролей.
- `--filesystem=xdg-download:ro` — выбрать свои `.jar` из «Загрузок».
