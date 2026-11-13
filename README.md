# MineHouse Launcher

Лаунчер сервера [MineHouse](https://www.minehouse.pro) на Rust + Tauri 2.
Одна кнопка «Играть»: вход через Microsoft, установка и обновление Java,
Minecraft, Fabric и сборки MineHouse, запуск сразу на `play.minehouse.pro`.

Нужна лицензия Minecraft: Java Edition, режима без входа нет.

## Как устроено

| Что | Откуда | Где в коде |
| --- | --- | --- |
| Вход | Microsoft OAuth (device code) → Xbox Live → XSTS → Minecraft services, проверка лицензии | `crates/core/src/auth.rs` |
| Java | Сначала ищет уже установленную Java нужной версии и разрядности (JAVA_HOME, PATH, папки JDK, официальный лаунчер, Prism; `mh-cli java`), иначе качает рантайм Mojang по компоненту из version JSON. Выключается в настройках | `java.rs` |
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

## Сборки и папка игры

Сборок может быть несколько (меню в шапке, `instances.rs`): у каждой своя папка
игры с модами, ресурспаками, шейдерами, настройками и мирами, а Java, Minecraft,
библиотеки и ассеты общие. Моды MineHouse ставятся в каждую сборку, индексы своих
модов и паков лежат в `<сборка>/.minehouse/`. Новая сборка, копия, переименование
и удаление; удаляются только папки, которые лаунчер создал сам. Разные сборки
можно запускать одновременно, каждую под своим ником (сервер пускает ник один раз):
занята только запущенная сборка, общие файлы ставятся по очереди, журнал
помечает строки сборкой.

Настройки лаунчера, аккаунт и список сборок (`instances.json`) всегда в
`%APPDATA%\MineHouse`, `~/Library/Application Support/MineHouse`,
`~/.local/share/minehouse` (переопределяется `MINEHOUSE_HOME`). Папку для игры
(общие файлы + `instances/`) можно выбрать при первом запуске и сменить в
настройках: лаунчер переносит всё туда (на другой диск сначала копирует, потом
удаляет старое). Непустая чужая папка получает подпапку `MineHouse`. Старая
единственная сборка остаётся в `game/`, пока папку не перенесут. Во Flatpak
папка не меняется. `mh-cli instances list|new|use|rm|dir` делает то же без окна.

### Horizon (лёгкая)

Тип сборки «Horizon (лёгкая)» (`horizon.rs`) запускает
[Horizon](https://github.com/horizon-rust/horizon), Minecraft 26.3 на Rust, с
нативными модами MineHouse (голосовой чат Plasmo Voice, Ping Wheel) из
[P5ina/minehouse-horizon](https://github.com/P5ina/minehouse-horizon). Шаги как в
`docs/api/launch-guide.md` Horizon:

1. `minehouse-horizon-<платформа>.zip` из последнего релиза minehouse-horizon
   (проверка по `SHA256SUMS`): наш `horizon-client` и `minehouse-horizon.json` с
   коммитом и Nightly Horizon, из которых он собран.
2. Эта Nightly Horizon для платформы, `SHA256SUMS` проверяется по подписи Ed25519
   (ключ из `release-keys.txt` Horizon). Если датированного релиза нет, берётся
   `nightly`, но только пока тег стоит на том же коммите. Распаковывается в
   `<данные>/horizon/builds/<nightly>-<коммит>/`, её `horizon-client` заменяется
   нашим. Хранятся текущая и одна предыдущая.
3. `horizon-prepare --check`; если ресурсов нет: ванильные файлы 26.3 (клиент,
   серверный jar, библиотеки, ассеты), Java 25, затем `horizon-prepare --prepare
   --complete --progress json` в `<данные>/horizon/resources/<коммит>/`. Первый раз
   несколько минут (на CI-машине ~700 МБ), дальше только проверка.
4. `horizon-client --resources … --game-dir <сборка> --home <сборка>
   --session-stdin --quickPlayMultiplayer play.minehouse.pro:25524`, сессия JSON
   в stdin. Коды выхода 1/2/101 показываются ошибкой с путём к отчёту.

Сборки есть для Windows x64, Linux x86_64 и macOS на Apple silicon (что собирает
CI minehouse-horizon). Кнопки «Моды» и «Паки» для Horizon скрыты.
`MINEHOUSE_HORIZON_ZIP=<zip>` берёт локальный zip (артефакт CI) вместо релиза,
`MINEHOUSE_HORIZON_URL` меняет адрес релизов, `mh-cli horizon` ставит и печатает
команду запуска.

При первом запуске показывается знакомство: папка, сборка, вход через Microsoft,
Discord (вступить, войти на сайт, заявка). Повторить можно из настроек.
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
Flatpak (свой репозиторий на GitHub Pages): [`flatpak/README.md`](flatpak/README.md).

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

### Подпись macOS (Developer ID + нотаризация)

Если в Settings → Secrets and variables → Actions заданы секреты ниже, релиз
подписывает `MineHouse.app` сертификатом Developer ID, отправляет его на
нотаризацию Apple, штампует тикет и проверяет результат (`codesign`, `spctl`,
`stapler`), в том числе внутри архива обновления `MineHouse.app.tar.gz`.
Архив обновления собирается уже из подписанного приложения и подписывается
ключом Tauri как раньше, так что автообновление не меняется. Без секретов
macOS-сборка остаётся неподписанной (в логе будет предупреждение).

| Секрет | Что это |
| --- | --- |
| `APPLE_CERTIFICATE` | сертификат Developer ID Application с ключом, `.p12` в base64 |
| `APPLE_CERTIFICATE_PASSWORD` | пароль, заданный при экспорте `.p12` |
| `APPLE_SIGNING_IDENTITY` | `Developer ID Application: Имя (TEAMID)` |
| `APPLE_ID` | email Apple ID аккаунта разработчика |
| `APPLE_PASSWORD` | app-specific password с appleid.apple.com |
| `APPLE_TEAM_ID` | Team ID (10 символов) |

Вместо `APPLE_ID`/`APPLE_PASSWORD`/`APPLE_TEAM_ID` можно дать ключ App Store
Connect API: `APPLE_API_KEY` (Key ID), `APPLE_API_ISSUER` (Issuer ID) и
`APPLE_API_KEY_P8` (содержимое файла `AuthKey_XXXX.p8`). Если заданы оба
варианта, используется ключ API.

Как получить сертификат:

1. На Mac: Связка ключей → Ассистент сертификации → Запросить сертификат у
   бюро сертификации → «Сохранено на диске».
2. <https://developer.apple.com/account/resources/certificates/add> →
   **Developer ID Application** (профиль G2 Sub-CA) → загрузить запрос →
   скачать `.cer` и открыть двойным кликом.
3. В Связке ключей (вкладка «Мои сертификаты») экспортировать сертификат
   вместе с ключом в `.p12` с паролем, затем `base64 -i cert.p12 | pbcopy`
   и вставить в `APPLE_CERTIFICATE`.
4. Точное имя для `APPLE_SIGNING_IDENTITY`: `security find-identity -v -p codesigning`.

Windows-установщик пока не подписан, SmartScreen при первом запуске предупредит.

---

MineHouse не является официальным продуктом Minecraft и не связан с Mojang или Microsoft.
