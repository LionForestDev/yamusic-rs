//! Linux: окно живёт в Slint/winit, системные действия — через xdg-open, zenity/kdialog
//! и файл автозапуска freedesktop.

use std::cell::RefCell;
use std::path::PathBuf;
use std::process::Command;

use slint::winit_030::WinitWindowAccessor;
use slint::ComponentHandle;

/// Оконная подсистема, выбранная winit. `WAYLAND_DISPLAY` непустой — Wayland.
/// Функция честно отвечает на вопрос «работает ли здесь скрытие окна»: на Wayland
/// оно невозможно (см. `prepare_backend`).
pub fn is_wayland() -> bool {
    std::env::var("WAYLAND_DISPLAY").map(|v| !v.is_empty()).unwrap_or(false)
}

/// Настоящее состояние окна по данным оконной системы. `None` — оконная подсистема
/// его не сообщает (Wayland), тогда вызывающий верит состоянию интерфейса.
fn winit_visible() -> Option<bool> {
    with_ui(|ui| ui.window().with_winit_window(|w| w.is_visible())).flatten().flatten()
}

fn winit_minimized() -> Option<bool> {
    with_ui(|ui| ui.window().with_winit_window(|w| w.is_minimized())).flatten().flatten()
}

/// Существует ли сокет X-сервера для `DISPLAY` вида `:0`, `:1.0`, `host:0`.
fn x_socket_exists(display: &str) -> bool {
    let Some((_, rest)) = display.rsplit_once(':') else { return false };
    let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if num.is_empty() {
        return false;
    }
    std::path::Path::new(&format!("/tmp/.X11-unix/X{num}")).exists()
}

/// Выбрать оконную подсистему ДО создания окна (вызывается из `main`).
///
/// Wayland физически не умеет прятать окно в трей: в winit 0.30 `set_visible(false)`
/// на Wayland пустой («Not possible on Wayland»), а `set_minimized(false)` игнорируется
/// («You can't unminimize the window on Wayland»), то есть свёрнутое окно оттуда уже не
/// вернуть — в том числе значком в трее. Поэтому на Wayland «скрыть в трей» выглядит
/// нерабочим, сколько ни правь интерфейс.
///
/// Через X11 (XWayland) те же вызовы работают по-настоящему: скрытие снимает окно с
/// экрана, показ и разворачивание возвращают его, панель задач и область без панелей
/// доступны. Поэтому при живой сессии Wayland и доступном X-сервере окно создаём в X11.
/// Отключить это можно переменной окружения `YAMUSIC_NO_X11=1`.
pub fn prepare_backend() {
    let wayland = is_wayland();
    let display = std::env::var("DISPLAY").map(|v| v.trim().to_string()).unwrap_or_default();
    if std::env::var_os("YAMUSIC_NO_X11").is_some() {
        crate::config::log(if wayland {
            "окно: Wayland по YAMUSIC_NO_X11 — прятать окно в трей здесь нельзя, только свернуть"
                .to_string()
        } else {
            "окно: X11 (YAMUSIC_NO_X11)".to_string()
        });
        return;
    }
    if !wayland {
        crate::config::log("окно: X11 — скрытие в трей и разворачивание работают".to_string());
        return;
    }
    if display.is_empty() {
        crate::config::log(
            "окно: Wayland, X-сервера нет (DISPLAY пуст) — окно не спрятать, останется сворачивание"
                .to_string(),
        );
        return;
    }
    if !x_socket_exists(&display) {
        crate::config::log(format!(
            "окно: Wayland, X-сервер {display} недоступен — окно не спрятать, останется сворачивание"
        ));
        return;
    }
    std::env::remove_var("WAYLAND_DISPLAY");
    std::env::remove_var("WAYLAND_SOCKET");
    crate::config::log(format!(
        "окно: X11 через XWayland (DISPLAY={display}) — скрытие в трей и возврат работают"
    ));
}

thread_local! {
    /// Окно приложения. Через него выполняются все оконные операции: на Linux у нас нет
    /// HWND, зато есть сам компонент Slint.
    static UI: RefCell<Option<slint::Weak<crate::gui::AppWindow>>> = const { RefCell::new(None) };
}

/// Запомнить окно приложения (вызывается один раз при запуске интерфейса).
pub fn register(ui: slint::Weak<crate::gui::AppWindow>) {
    UI.with(|u| *u.borrow_mut() = Some(ui));
}

fn with_ui<R>(f: impl FnOnce(&crate::gui::AppWindow) -> R) -> Option<R> {
    UI.with(|u| u.borrow().as_ref().and_then(|w| w.upgrade()).map(|ui| f(&ui)))
}

pub fn is_visible(_h: isize) -> bool {
    // X11 сообщает настоящее состояние окна. Wayland его не сообщает (None) — там
    // верим состоянию интерфейса, другого источника нет.
    if let Some(v) = winit_visible() {
        return v;
    }
    with_ui(|ui| ui.window().is_visible()).unwrap_or(false)
}

pub fn is_iconic(_h: isize) -> bool {
    if let Some(v) = winit_minimized() {
        return v;
    }
    with_ui(|ui| ui.window().is_minimized()).unwrap_or(false)
}

pub fn show(_h: isize) {
    with_ui(|ui| {
        let w = ui.window();
        // На X11 разворачивание работает; на Wayland winit его игнорирует (тогда окно
        // возвращает композитор — из панели задач).
        w.set_minimized(false);
        let _ = ui.show();
        w.with_winit_window(|ww| ww.focus_window());
    });
}

pub fn hide(_h: isize) {
    if is_wayland() {
        // Wayland не умеет прятать окно: winit игнорирует set_visible(false). Честно
        // делаем то, что здесь возможно, — сворачиваем в панель задач.
        with_ui(|ui| ui.window().set_minimized(true));
        crate::config::log(
            "скрытие: Wayland не прячет окно — свернули в панель задач".to_string(),
        );
        return;
    }
    with_ui(|ui| {
        let _ = ui.window().hide();
    });
}

pub fn force_foreground(_h: isize) {
    with_ui(|ui| ui.window().with_winit_window(|w| w.focus_window()));
}

pub fn dpi_scale(_h: isize) -> f32 {
    with_ui(|ui| ui.window().scale_factor()).unwrap_or(1.0)
}

/// Значок окна на Linux берётся из `.desktop` (поле `Icon`), отдельная установка не нужна.
pub fn set_icon(_h: isize) {}

/// Скруглять углы — работа композитора, приложению делать нечего.
pub fn round_corners(_h: isize) {}

/// Рабочая область монитора без панелей, в физических пикселях: лево, верх, право, низ.
pub fn work_area(_h: isize) -> (i32, i32, i32, i32) {
    if let Some(a) = x11_workarea() {
        return a;
    }
    // Wayland: только геометрия монитора, панель не вычитается.
    let monitor = with_ui(|ui| {
        ui.window().with_winit_window(|w| {
            w.current_monitor().map(|m| {
                let (p, s) = (m.position(), m.size());
                (p.x, p.y, p.x + s.width as i32, p.y + s.height as i32)
            })
        })
    });
    monitor.flatten().flatten().unwrap_or((0, 0, 1920, 1080))
}

/// `_NET_WORKAREA` корневого окна X11 — единственный способ узнать область без панелей.
fn x11_workarea() -> Option<(i32, i32, i32, i32)> {
    std::env::var_os("DISPLAY")?;
    let out = Command::new("xprop").args(["-root", "_NET_WORKAREA"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let nums: Vec<i32> = text
        .split_once('=')
        .map(|(_, rest)| rest)
        .unwrap_or(&text)
        .split(',')
        .filter_map(|v| v.trim().parse::<i32>().ok())
        .collect();
    if nums.len() < 4 {
        return None;
    }
    let (x, y, w, h) = (nums[0], nums[1], nums[2], nums[3]);
    if w <= 0 || h <= 0 {
        return None;
    }
    Some((x, y, x + w, y + h))
}

/// Открыть ссылку в браузере. Перебираем все известные способы: xdg-open есть не везде
/// (а без него кнопка «Открыть ya.ru/device» молча ничего не делает).
pub fn open_url(url: &str) {
    let candidates: [(&str, &[&str]); 7] = [
        ("xdg-open", &[]),
        ("gio", &["open"]),
        ("kde-open6", &[]),
        ("kde-open5", &[]),
        ("gnome-open", &[]),
        ("sensible-browser", &[]),
        ("x-www-browser", &[]),
    ];
    for (program, args) in candidates {
        let spawned = Command::new(program).args(args).arg(url).spawn();
        if spawned.is_ok() {
            return;
        }
    }
    crate::config::log(format!(
        "не удалось открыть ссылку {url}: нет ни xdg-open, ни gio, ни kde-open (нужен пакет xdg-utils)"
    ));
}

/// Linux сам освобождает память процесса — принудительно сжимать нечего.
pub fn trim_memory() {}

pub fn release_memory() {}

/// Светлая ли тема оформления. Спрашиваем GNOME, затем KDE; по умолчанию — светлая.
pub fn system_light_theme() -> bool {
    if let Ok(o) = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "color-scheme"])
        .output()
    {
        if o.status.success() {
            let s = String::from_utf8_lossy(&o.stdout).to_lowercase();
            if s.contains("prefer-dark") {
                return false;
            }
            if s.contains("prefer-light") || s.contains("default") {
                return true;
            }
        }
    }
    for tool in ["kreadconfig6", "kreadconfig5"] {
        if let Ok(o) = Command::new(tool)
            .args(["--file", "kdeglobals", "--group", "General", "--key", "ColorScheme"])
            .output()
        {
            if o.status.success() {
                let s = String::from_utf8_lossy(&o.stdout).to_lowercase();
                if s.contains("dark") {
                    return false;
                }
                if !s.trim().is_empty() {
                    return true;
                }
            }
        }
    }
    true
}

fn config_home() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
}

/// Файл автозапуска пользователя (freedesktop: работает в GNOME, KDE, XFCE).
pub fn autostart_path() -> Option<PathBuf> {
    Some(config_home()?.join("autostart").join("yamusic-rs.desktop"))
}

fn autostart_text() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    Some(format!(
        "[Desktop Entry]\nType=Application\nName=Яндекс Музыка RS\nComment=Неофициальный клиент Яндекс Музыки\nExec=\"{}\" --tray\nIcon=yamusic-rs\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
        exe.display()
    ))
}

pub fn autostart_enabled() -> bool {
    autostart_path().map(|p| p.is_file()).unwrap_or(false)
}

/// Включить или выключить автозапуск: пишем/удаляем личный файл в `~/.config/autostart`.
pub fn set_autostart(on: bool) -> bool {
    let Some(path) = autostart_path() else { return false };
    if !on {
        return match std::fs::remove_file(&path) {
            Ok(()) => true,
            Err(e) => e.kind() == std::io::ErrorKind::NotFound,
        };
    }
    let Some(text) = autostart_text() else { return false };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(&path, text).is_ok()
}

/// Клиент перенесли в другую папку — поправить путь в автозапуске, если он включён.
pub fn refresh_autostart() {
    if autostart_enabled() {
        set_autostart(true);
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
pub fn is_window(_h: isize) -> bool {
    with_ui(|_| true).unwrap_or(false)
}

/// Системный диалог выбора картинки. `None` — пользователь отказался или диалога нет.
pub fn pick_image(_owner: isize, title: &str) -> Option<String> {
    if let Some(path) = zenity(&[
        "--file-selection",
        "--title",
        title,
        "--file-filter=Изображения | *.jpg *.jpeg *.png",
    ]) {
        return Some(path);
    }
    kdialog(&["--title", title, "--getopenfilename", ".", "*.jpg *.jpeg *.png"])
}

/// Системный диалог выбора папки, открывается на `start`.
pub fn pick_folder(_owner: isize, title: &str, start: &str) -> Option<String> {
    let filter = format!("--filename={}/", start.trim_end_matches('/'));
    let args = ["--file-selection", "--directory", "--title", title, filter.as_str()];
    if let Some(path) = zenity(&args) {
        return Some(path);
    }
    let kdialog_start = if start.is_empty() { "." } else { start };
    kdialog(&["--title", title, "--getexistingdirectory", kdialog_start])
}

fn zenity(args: &[&str]) -> Option<String> {
    let out = Command::new("zenity").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

fn kdialog(args: &[&str]) -> Option<String> {
    let out = Command::new("kdialog").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// Второй запуск просит показать окно: поток сокета ставит флаг, интерфейс его снимает.
static WAKE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn request_wake() {
    WAKE.store(true, std::sync::atomic::Ordering::Release);
}

/// Забрать запрос «показать окно». Вызывается из интерфейса, поэтому выставлять окно
/// можно прямо в потоке UI — без пересылки между потоками.
pub fn take_wake() -> bool {
    WAKE.swap(false, std::sync::atomic::Ordering::AcqRel)
}
