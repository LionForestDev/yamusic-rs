//! Локальные копии треков: где лежат, есть ли трек на диске, чтение с диска.
//!
//! Один и тот же расчёт имени файла используется и при скачивании, и при поиске готового
//! файла перед воспроизведением — иначе скачанный трек не найдётся и снова полезет в сеть.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::api::Track;
use crate::config::Config;

/// Каталог скачивания. Ставится при старте и при смене папки в настройках; по нему же
/// аудиопоток ищет готовый файл, чтобы не тянуть трек из сети.
static DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn set_dir(p: PathBuf) {
    *DIR.lock().unwrap_or_else(|e| e.into_inner()) = Some(p);
}

pub fn current_dir() -> Option<PathBuf> {
    DIR.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Куда качаем по умолчанию: «Музыка/Яндекс Музыка» в домашнем каталоге.
pub fn default_dir() -> PathBuf {
    if cfg!(windows) {
        let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into());
        PathBuf::from(home).join("Music").join("Яндекс Музыка")
    } else {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        PathBuf::from(home).join("Music").join("Яндекс Музыка")
    }
}

/// Папка скачивания: из настроек, иначе значение по умолчанию.
pub fn music_dir(cfg: &Config) -> PathBuf {
    if !cfg.download_dir.is_empty() {
        return cfg.download_dir.clone().into();
    }
    default_dir()
}

/// Имя файла или папки, которое Windows точно примет: без запрещённых и управляющих
/// символов, без точки и пробела в конце, не зарезервированное (CON, NUL, COM1…), не длиннее `max`.
pub fn file_name_max(s: &str, max: usize) -> String {
    let mut out: String = s
        .chars()
        .map(|c| if "\\/:*?\"<>|".contains(c) || (c as u32) < 0x20 { '_' } else { c })
        .collect();
    if out.chars().count() > max {
        out = out.chars().take(max).collect();
    }
    let out = out.trim_end_matches(['.', ' ']).trim_start().to_string();
    if out.is_empty() {
        return "_".into();
    }
    let stem = out.split('.').next().unwrap_or("").trim_end().to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.as_bytes()[3].is_ascii_digit());
    if reserved {
        format!("_{out}")
    } else {
        out
    }
}

pub fn file_name(s: &str) -> String {
    file_name_max(s, 100)
}

/// Расширение по первым байтам: mp3, AAC (ADTS), MP4 или FLAC.
pub fn audio_ext(b: &[u8]) -> &'static str {
    match b {
        [b'I', b'D', b'3', ..] => "mp3",
        [_, _, _, _, b'f', b't', b'y', b'p', ..] => "m4a",
        [b'f', b'L', b'a', b'C', ..] => "flac",
        [0xFF, x, ..] if x & 0xF6 == 0xF0 => "aac",
        _ => "mp3",
    }
}

/// Имя трека без расширения — одинаковое у скачивания и у поиска.
pub fn track_file_stem(t: &Track) -> String {
    file_name_max(&format!("{} — {}", t.artists, t.title), 120)
}

const EXTS: [&str; 4] = ["mp3", "m4a", "aac", "flac"];

/// Готовый файл трека в каталоге: не `.part`, ненулевого размера.
pub fn find_in(dir: &Path, t: &Track) -> Option<PathBuf> {
    let stem = track_file_stem(t);
    EXTS.iter().map(|e| dir.join(format!("{stem}.{e}"))).find(|p| {
        std::fs::metadata(p).is_ok_and(|m| m.len() > 0)
    })
}

/// Трек уже скачан (по текущему каталогу скачивания).
pub fn saved_already(dir: &Path, t: &Track) -> bool {
    find_in(dir, t).is_some()
}

/// Есть ли трек на диске в текущем каталоге скачивания.
pub fn is_local(t: &Track) -> bool {
    current_dir().is_some_and(|d| find_in(&d, t).is_some())
}

/// Прочитать трек с диска, если он скачан.
pub fn read(t: &Track) -> Option<(PathBuf, Vec<u8>)> {
    let dir = current_dir()?;
    let path = find_in(&dir, t)?;
    let bytes = std::fs::read(&path).ok()?;
    if bytes.is_empty() {
        return None;
    }
    Some((path, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track() -> Track {
        Track {
            id: "57703".into(),
            album_id: None,
            title: "Трек".into(),
            version: String::new(),
            artists: "Артист".into(),
            artist_list: Vec::new(),
            duration_ms: 1000,
            cover: None,
            explicit: false,
            lyrics: false,
            color: None,
            album_title: String::new(),
            year: None,
            position: None,
        }
    }

    #[test]
    fn скачанный_трек_находится_и_читается() {
        let dir = std::env::temp_dir().join(format!("yamusic-local-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        set_dir(dir.clone());
        let t = track();
        assert!(!is_local(&t), "до скачивания файла на диске нет");
        let name = format!("{}.mp3", track_file_stem(&t));
        std::fs::write(dir.join(&name), b"ID3test-bytes").unwrap();
        assert!(is_local(&t), "скачанный файл {name} не найден");
        let (p, b) = read(&t).expect("скачанный файл читается");
        assert_eq!(p.file_name().unwrap().to_string_lossy(), name);
        assert_eq!(b, b"ID3test-bytes");
        // Пустой файл и обрывок .part скачанными не считаются.
        let mut other = track();
        other.id = "57704".into();
        other.title = "Другой".into();
        std::fs::write(dir.join(format!("{}.mp3", track_file_stem(&other))), b"").unwrap();
        assert!(!is_local(&other), "пустой файл не считается скачанным");
        std::fs::write(dir.join(format!("{}.part", track_file_stem(&other))), b"x").unwrap();
        assert!(!is_local(&other), "обрывок .part не считается скачанным");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
