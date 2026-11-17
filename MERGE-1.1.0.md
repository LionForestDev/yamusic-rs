# Перенос наших доработок на 1.1.0 (r12) — план и разведданные

Состояние: r11 (ветка main, коммит 80635c0) = апстрим 1.0.2 + все наши правки. Собран, проверен, выдан клиенту.
Апстрим: origin/main = v1.1.0 (72f17c7 + README).

## Что даёт 1.1.0

MPRIS, src/sys/{linux,windows}.rs (вместо win.rs), токен в связке ключей, XDG-пути,
Discord через unix-сокет, AppImage/deb/rpm + PKGBUILD, популярные треки, смена темы на ходу,
подробный лог, ярлык автозапуска «вместе с системой» (наш аналог уже не нужен).

## Чего в 1.1.0 НЕТ (нужно перенести — иначе регресс)

1. Дизлайк: у них `enabled: App.is-wave` в ui/wave.slint и `enabled: on && App.is-wave` в ui/playerbar.slint → ставим `App.has-track` и `on`.
2. Скачанные треки: src/local.rs, зелёная стрелка (ui/rows.slint + токен в ui/theme.slint), поле `downloaded` в TrackItem (ui/state.slint), is_local в track_item, set_dir/лог в run(), обновление галочек после скачивания (apply_filter).
3. Потоковое воспроизведение: src/stream.rs, api::open_stream, в player.rs — Msg::Stream, start_download (локальный файл → диск, иначе поток с 1 МБ), start_full_download, play_stream, кэш трёх треков, prefetch после 25 % прослушивания, replay через start_current.
4. Фикс указателя после перетаскивания окна (r5): POINTER_STUCK, dispatch_pointer_release, schedule_pointer_release + правки в on_drag_window/on_cursor.
5. Компактный режим: MINI_MIN_W = 152, apply_mini_geometry, sync_window_size, возврат размера при выходе (3 захода: сразу/100 мс/400 мс).
6. Трей на Linux (src/tray/linux.rs, StatusNotifierItem). В 1.1.0 трея на Linux НЕТ вообще —
   при наивном «взять их версию» значок в трее и скрытие окна в него пропадут.

## Как мержить

`git merge v1.1.0` → 11 конфликтов:
- взять theirs: Cargo.lock, Cargo.toml, src/config.rs, src/discord.rs, src/mpris.rs (их MPRIS богаче нашего), src/secret.rs, ui/account.slint (их подпись автозапуска).
- src/main.rs: их структура (sys::claim_instance, sys::wake_existing, sys::refresh_autostart, sys::audio_identity) + `mod local; mod stream;` + BUILD_STAMP + строка лога сборки.
- ui/state.slint: их свойства (linux, translations) + наше поле downloaded.
- src/player.rs: брать НАШ файл (их дельта — косметика: замыкание skip в fetch_batch), затем сверху их 3 строки.
- src/api.rs: их + open_stream.
- src/gui.rs: 8 конфликтов — win::* → sys::* (work_area, hide, created, open_url, copy_text), плюс наши блоки из п.4 и п.5.
- Переименование: их src/sys/windows.rs вместо src/win.rs; наш src/win/{mod,linux,windows}.rs → заменить на sys::, вызовы win::* переписать.

## Проверка перед выдачей

cargo build --release; cargo test --release --bin yamusic-rs -- local:: stream::; запуск на weston (метка r12 в первой строке),
md5 бинарника в архиве = собранному; глазами — стрелка скачанного, дизлайк на главной, компактный режим и его возврат, трей.
