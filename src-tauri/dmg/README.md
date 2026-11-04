# Фон окна установщика macOS

`background.tiff` — фон окна dmg (660×420, внутри две картинки: 1x и 2x для Retina).
Подключён в `tauri.conf.json` → `bundle.macOS.dmg`, там же позиции иконок.

Исходник — `background.html` (шрифт Geist из `node_modules`, нужен `pnpm install`).
Пересобрать: снять скриншоты страницы 660×420 при deviceScaleFactor 1 и 2
(например Playwright), затем

    convert bg.png -units PixelsPerInch -density 72 -alpha off a.tif
    convert bg@2x.png -units PixelsPerInch -density 144 -alpha off b.tif
    convert a.tif b.tif -compress lzw background.tiff

Тёмная плашка под иконкой приложения нарисована в фоне: иконка лаунчера — белая
буква без подложки и на светлом фоне иначе не видна.
