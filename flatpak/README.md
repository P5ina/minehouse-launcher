# Flatpak

App ID `pro.minehouse.launcher` (совпадает с `identifier` в `tauri.conf.json`).
Лаунчер раздаётся из своего Flatpak-репозитория на GitHub Pages, не с Flathub.

Установка для игроков:

```sh
flatpak install --user https://p5ina.github.io/minehouse-launcher/pro.minehouse.launcher.flatpakref
flatpak update   # обновления
```

| Файл | Что |
| --- | --- |
| `pro.minehouse.launcher.yml` | Манифест: перепаковывает `.deb` релиза (так советует Tauri), рантайм `org.gnome.Platform` (с WebKitGTK 4.1) с Flathub |
| `pro.minehouse.launcher.metainfo.xml` | AppStream: описание, скриншоты, история версий. Попадает в `.deb` (`bundle.linux.deb.files`), оттуда во Flatpak |
| `publish.sh` | Сборка + подписанный OSTree-репозиторий, `.flatpakref`, `.flatpakrepo`, `.flatpak`-бандл, `index.html` |
| `minehouse-flatpak.asc` | Публичный GPG-ключ репозитория (он же зашит в `.flatpakref`) |
| `../.github/workflows/flatpak.yml` | После каждого успешного Release: `publish.sh` → GitHub Pages, бандл в релиз |

Внутри Flatpak встроенное автообновление выключено (`in_flatpak()` в
`src-tauri/src/main.rs`: плагин updater не подключается, баннер не показывается),
обновляет `flatpak update` из нашего репозитория.

## Настройка один раз

1. Секрет `FLATPAK_GPG_PRIVATE_KEY`: приватный ключ к `minehouse-flatpak.asc` (ASCII armor, без пароля).
   Если ключ потерян, новый ключ = игрокам придётся переустановить remote.
2. Settings → Pages → Source: **GitHub Actions**.
3. Первую публикацию без нового релиза: Actions → Flatpak → Run workflow.

## Новая версия

1. Добавить `<release version="X.Y.Z" date="…">` в metainfo **до** релиза.
2. Выпустить релиз как обычно (Actions → Release), Flatpak опубликуется сам.

Репозиторий каждый раз собирается заново с одним коммитом: клиенты просто
скачивают новый, подпись проверяется тем же ключом.

## Собрать локально

```sh
pnpm tauri build --bundles deb
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
gpg --quick-gen-key "test" rsa4096 sign never   # любой ключ для проверки
flatpak/publish.sh target/release/bundle/deb/MineHouse_<версия>_amd64.deb <версия> <id ключа> http://localhost:8000 site
(cd site && python3 -m http.server 8000) &
flatpak install --user http://localhost:8000/pro.minehouse.launcher.flatpakref
```

## Права

- `--socket=wayland` + `--socket=fallback-x11`: окно лаунчера (WebKitGTK) и Minecraft 26.x
  (LWJGL 3.4 на SDL3, собран с Wayland) работают на Wayland, X11 только для X11-сессий.
- `--device=dri` — OpenGL для игры, `--socket=pulseaudio` — звук и микрофон для голосового чата.
- `--talk-name=org.freedesktop.secrets` — refresh-токен Microsoft в хранилище паролей.
- `--filesystem=xdg-download:ro` — выбрать свои `.jar` из «Загрузок».
