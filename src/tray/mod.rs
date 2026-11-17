//! Значок в области уведомлений и меню плеера.
//!
//! Windows — Win32 (`windows.rs`, Shell_NotifyIcon + медиаклавиши). На Linux трея пока нет:
//! там же, где он появится (StatusNotifierItem), будет и реализация `linux.rs`.

#[cfg(windows)]
#[path = "windows.rs"]
mod imp;

#[cfg(not(windows))]
#[path = "linux.rs"]
mod imp;

pub use imp::*;
