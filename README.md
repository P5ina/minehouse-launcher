# MineHouse Launcher

Лаунчер сервера [MineHouse](https://www.minehouse.pro) на Rust + Tauri 2.
Одна кнопка «Играть»: вход через Microsoft, установка и обновление Java,
Minecraft, Fabric и сборки MineHouse, запуск сразу на `play.minehouse.pro`.

Нужна лицензия Minecraft: Java Edition, режима без входа нет.

## Как устроено

| Что | Откуда | Где в коде |
| --- | --- | --- |
| Вход | Microsoft OAuth (device code) → Xbox Live → XSTS → Minecraft services, проверка лицензии | `crates/core/src/auth.rs` |
| Java | Рантайм Mojang (тот же, что у официального лаунчера), компонент из version JSON | `java.rs` |
| Minecraft | `piston-meta.mojang.com`: клиент, библиотеки, ассеты, log4j-конфиг | `minecraft.rs` |
| Fabric | Профиль с `meta.fabricmc.net`, сливается поверх ванильной версии | `fabric.rs` |
| Сборка | `https://www.minehouse.pro/modpack/manifest.json` (sha512 каждого файла) | `modpack.rs` |
| Свои моды | Modrinth API (Fabric + версия игры из manifest) и свои `.jar` | `user_mods.rs`, `modrinth.rs` |
| Запуск | Аргументы из version JSON, `--quickPlayMultiplayer play.minehouse.pro` | `launch.rs` |
| Онлайн | Server List Ping | `ping.rs` |

Версию Minecraft и Fabric лаунчер берёт из `manifest.json` сайта (поля
`minecraft`, `loader_version`), поэтому для обновления игры достаточно
поменять `modpack/pack.json` в `minehouse-landing`. Каждый запуск сверяет
файлы и докачивает только изменённые. Состояние сборки пишется в
`config/minehouse-pack.json`, как у автообновления мода MineHouse Menu,
поэтому они не мешают друг другу.

## Свои моды

Кнопка «Моды»: поиск на Modrinth (только Fabric под версию сборки, с
обязательными зависимостями), свои `.jar` кнопкой или перетаскиванием в окно,
включение/выключение (файл переименовывается в `.jar.disabled`), удаление,
обновление. Моды лежат в той же `mods/`, но обновление сборки трогает только
файлы из manifest, а список своих модов хранится в `user-mods.json`.

Чтобы игрока пустило на сервер, перед установкой и перед каждым запуском лаунчер
проверяет:

- мод уже есть в сборке (по проекту Modrinth или Fabric id) → не ставит / выключает;
- мод есть на сервере (`server_mods` в manifest, его пишет
  `modpack/server-mods.json` в minehouse-landing) → ставит ровно серверную
  версию, а мод с тем же Fabric id из другого проекта (FSit против FSit
  Continued) не пускает;
- мод только для сервера или требует серверную часть, которой у MineHouse нет → не ставит;
- сменилась версия Minecraft → обновляет моды с Modrinth, а если версии нет, выключает.

`mh-cli mods list|search|add|jar|rm|update|sync` делает то же без окна;
`MINEHOUSE_MANIFEST_URL` подменяет manifest для проверки сборки до выкладки.

Всё лежит в отдельной папке и не пересекается с другими лаунчерами:
`%APPDATA%\MineHouse`, `~/Library/Application Support/MineHouse`,
`~/.local/share/minehouse` (переопределяется `MINEHOUSE_HOME`).
Refresh-токен Microsoft хранится в системном хранилище паролей.

## Разработка

```sh
pnpm install
pnpm tauri dev                      # окно лаунчера
cargo test --workspace
cargo run -p minehouse-core --bin mh-cli -- install   # скачать и проверить всё без окна
```

Интерфейс (`ui/`) на React + Vite + Tailwind и компонентах shadcn/ui в той же теме,
что сайт (zinc, Geist). Фоновые эффекты из React Bits Pro лежат в
`ui/src/components/react-bits`. Новые компоненты: `pnpm exec shadcn add <имя>`.
`pnpm tauri dev` сам запускает Vite, `pnpm tauri build` собирает `ui/dist`;
для одного `cargo build`/`clippy` сначала нужен `pnpm build`.

Linux: нужны `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev`.

## Вход через Microsoft: настройка один раз

1. [Azure Portal → App registrations](https://portal.azure.com/#view/Microsoft_AAD_RegisteredApps/ApplicationsListBlade) → New registration,
   «Personal Microsoft accounts only». В Authentication включить «Allow public client flows».
2. Отправить Application (client) ID на одобрение Minecraft API: <https://aka.ms/mce-reviewappid>.
   Пока Mojang не одобрит, вход останавливается на шаге Minecraft (403).
3. Client ID записан в `crates/core/src/auth.rs` (`DEFAULT_CLIENT_ID`, сейчас приложение
   «MineHouse Launcher» `f33e99c2-…`). Переопределить можно переменной `MINEHOUSE_MS_CLIENT_ID`.

## Релиз и автообновление

1. Поднять `version` в `Cargo.toml`.
2. Actions → Release → Run workflow (или `git tag v0.2.0 && git push --tags`).

Workflow `release.yml` собирает установщики (Windows NSIS, macOS dmg arm/intel,
Linux AppImage/deb), подписывает обновления ключом из секрета
`TAURI_SIGNING_PRIVATE_KEY` и публикует `latest.json` в релиз. Запущенный
лаунчер проверяет `releases/latest/download/latest.json` и предлагает обновиться.
Публичный ключ записан в `src-tauri/tauri.conf.json`.

Установщики не подписаны сертификатами Apple/Microsoft, поэтому при первом
запуске SmartScreen/Gatekeeper предупредят. Подпись можно добавить позже.

---

MineHouse не является официальным продуктом Minecraft и не связан с Mojang или Microsoft.
