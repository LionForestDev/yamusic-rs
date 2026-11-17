//! Тонкие обёртки над оконной системой: показать/спрятать окно, фокус, мониторы,
//! автозапуск, открытие ссылок.
//!
//! Windows — Win32 (`windows.rs`), Linux — Slint/winit плюс freedesktop-утилиты (`linux.rs`).
//! Публичный интерфейс один и тот же, поэтому остальной код платформу не различает.

#[cfg(windows)]
#[path = "windows.rs"]
mod imp;

#[cfg(not(windows))]
#[path = "linux.rs"]
mod imp;

pub use imp::*;

/// Можно ли спрятать окно в трей и вернуть оттуда значком. На Linux значок зависит от
/// рабочего стола (StatusNotifierItem), поэтому это проверка на месте, а не константа.
#[cfg(windows)]
pub fn hide_to_tray() -> bool {
    true
}

#[cfg(not(windows))]
pub fn hide_to_tray() -> bool {
    crate::tray::available()
}
