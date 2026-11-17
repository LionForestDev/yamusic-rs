//! Яндекс Музыка RS — неофициальный клиент Яндекс Музыки. Точка входа: один экземпляр
//! на пользователя, выбор рендерера, запуск окна.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod assets;
mod auth;
mod client;
mod config;
mod discord;
mod eq;
mod gui;
mod i18n;
mod images;
mod landing;
mod local;
#[cfg(not(windows))]
mod mpris;
mod net;
mod palette;
mod player;
mod secret;
mod stream;
mod tags;
mod tray;
mod update;
mod win;
mod yandex_id;

/// Отметка сборки: по ней в журнале видно, какая копия запущена.
#[cfg(not(windows))]
const BUILD_STAMP: &str = "linux r20";

/// Занять мьютекс единственного экземпляра. Если он уже чей-то — отпустить свою ссылку,
/// чтобы мьютекс исчез вместе с тем процессом.
#[cfg(windows)]
fn claim_instance() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::CreateMutexW;
    let name = win::wide(r"Local\yamusic_rs_single_instance");
    unsafe {
        let m = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            CloseHandle(m);
            return false;
        }
        true
    }
}

/// Linux: вместо мьютекса — сокет в папке настроек. Занятый сокет означает, что приложение
/// уже работает; тогда второму запуску достаточно попросить первый показать окно.
#[cfg(not(windows))]
fn claim_instance() -> bool {
    use std::io::Write as _;
    use std::os::unix::net::{UnixListener, UnixStream};

    // Папку настроек создаём заранее: без неё bind сокета падает и защита от второго
    // запуска молча не включается.
    let _ = std::fs::create_dir_all(config::dir());
    let path = config::dir().join("instance.sock");
    if let Ok(mut s) = UnixStream::connect(&path) {
        let _ = s.write_all(b"wake");
        let _ = s.flush();
        return false;
    }
    // Сокет остался от аварийно завершившегося процесса — убираем.
    let _ = std::fs::remove_file(&path);
    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            config::log(format!("сокет единственного экземпляра не создан: {e}"));
            return true;
        }
    };
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            win::request_wake();
            drop(conn);
        }
    });
    true
}

fn main() -> Result<(), slint::PlatformError> {
    #[cfg(not(windows))]
    config::log(format!("сборка {} Linux, {BUILD_STAMP}", env!("CARGO_PKG_VERSION")));
    // После обновления старая версия ещё закрывается: ждём её до 15 секунд.
    let after_update = std::env::args().any(|a| a == update::AFTER_UPDATE);
    let mut claimed = claim_instance();
    for _ in 0..60 {
        if claimed || !after_update {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
        claimed = claim_instance();
    }
    if !claimed {
        // Второй запуск просто показывает уже открытое окно.
        if tray::wake_existing() {
            return Ok(());
        }
        #[cfg(windows)]
        {
            let h: isize = std::fs::read_to_string(config::dir().join("hwnd")).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
            if h != 0 && win::is_window(h) {
                win::show(h);
                win::force_foreground(h);
            }
        }
        return Ok(());
    }
    // Skia без видеокарты: память не зависит от драйвера (с OpenGL или D3D окно занимает
    // в 2–3 раза больше). Режим работы («Плавный» или «Лёгкий») меняет только анимации
    // и кэши и переключается на лету (gui::apply_mode).
    #[cfg(not(windows))]
    win::prepare_backend();
    slint::BackendSelector::new().backend_name("winit".into()).renderer_name("skia-software".into()).select()?;
    update::cleanup();
    win::refresh_autostart();
    // «Спрятанным» запускаемся только при автозапуске; если трея нет, окно покажет сам
    // интерфейс (см. gui::run) — иначе приложение оказалось бы без окна и без значка.
    let start_hidden = std::env::args().any(|a| a == "--tray");
    gui::run(start_hidden)
}
