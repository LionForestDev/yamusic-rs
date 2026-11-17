//! Локальные копии треков: где лежат, есть ли трек на диске, чтение с диска.
//!
//! Каталог скачивания — не единственное место, где могут лежать файлы: старые сборки
//! считали путь от каталога запуска, у кого-то музыка лежит в «Музыка» вместо «Music».
//! Поэтому ищем трек сразу по нескольким каталогам, а для скорости держим индекс «имя → путь».

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::api::Track;
use crate::config::Config;

const EXTS: [&str; 4] = ["mp3", "m4a", "aac", "flac"];

/// Каталоги поиска скачанных треков (первый — тот, куда качаем).
static DIRS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
/// Индекс: нормализованное имя файла без расширения → путь. Строится лениво.
static INDEX: Mutex<Option<HashMap<String, PathBuf>>> = Mutex::new(None);

/// Каталог, куда складываем скачанное.
pub fn set_dir(p: PathBuf) {
    let mut d = DIRS.lock().unwrap_or_else(|e| e.into_inner());
    if d.first() != Some(&p) {
        d.insert(0, p);
        *INDEX.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// Список каталогов поиска: первый — куда качаем.
pub fn set_dirs(list: Vec<PathBuf>) {
    let mut seen = Vec::new();
    for p in list {
        if !seen.contains(&p) {
            seen.push(p);
        }
    }
    *DIRS.lock().unwrap_or_else(|e| e.into_inner()) = seen;
    *INDEX.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

pub fn dirs() -> Vec<PathBuf> {
    DIRS.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Каталог скачивания из настроек или по умолчанию.
pub fn music_dir(cfg: &Config) -> PathBuf {
    if !cfg.download_dir.is_empty() {
        return cfg.download_dir.clone().into();
    }
    default_dir()
}

/// Куда качаем по умолчанию: «Яндекс Музыка» в каталоге музыки из XDG (у русской локали это
/// часто «Музыка», а не «Music»), иначе «Music» в домашнем каталоге.
pub fn default_dir() -> PathBuf {
    if cfg!(windows) {
        let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into());
        return PathBuf::from(home).join("Music").join("Яндекс Музыка");
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let base = xdg_music_dir().unwrap_or_else(|| PathBuf::from(home).join("Music"));
    base.join("Яндекс Музыка")
}

/// Все места, где могут лежать скачанные треки: настройка, XDG-музыка, Music, Музыка,
/// а также старый путь «от каталога запуска».
pub fn candidates(cfg: &Config) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut push = |p: PathBuf| {
        if !out.contains(&p) {
            out.push(p);
        }
    };
    let sub = |base: PathBuf| base.join("Яндекс Музыка");

    if !cfg.download_dir.is_empty() {
        let p = PathBuf::from(&cfg.download_dir);
        push(p.clone());
        push(sub(p));
    }
    let home = if cfg!(windows) {
        PathBuf::from(std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into()))
    } else {
        PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
    };
    if let Some(xdg) = xdg_music_dir() {
        push(xdg.clone());
        push(sub(xdg));
    }
    for name in ["Music", "Музыка"] {
        let base = home.join(name);
        push(sub(base.clone()));
        push(base);
    }
    if let Ok(cwd) = std::env::current_dir() {
        push(sub(cwd.join("Music")));
        push(cwd.join("Music"));
    }
    out
}

/// Каталог музыки из ~/.config/user-dirs.dirs (локаль часто даёт «Музыка», а не «Music»).
fn xdg_music_dir() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let cfg = Path::new(&home).join(".config").join("user-dirs.dirs");
    let text = std::fs::read_to_string(cfg).ok()?;
    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with("XDG_MUSIC_DIR=") {
            continue;
        }
        let raw = line.trim_start_matches("XDG_MUSIC_DIR=").trim().trim_matches('"');
        let path = raw.replace("$HOME", &home);
        if !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }
    None
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

/// Ключ поиска: без регистра, лишних пробелов и различий в тире.
fn key(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars().flat_map(|c| c.to_lowercase()) {
        let c = match c {
            '–' | '—' | '−' => '-',
            _ => c,
        };
        if c.is_whitespace() {
            space = true;
            continue;
        }
        if space && !out.is_empty() {
            out.push(' ');
        }
        space = false;
        out.push(c);
    }
    out
}

/// Насколько глубоко смотрим внутрь каталога музыки. Скачанный список — это подпапка
/// («Мне нравится», название плейлиста), поэтому одного уровня недостаточно.
const MAX_DEPTH: usize = 3;

fn is_audio(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()).is_some_and(|ext| EXTS.iter().any(|x| x.eq_ignore_ascii_case(ext)))
}

/// Обойти каталог и вложенные подпапки, собрать «имя без расширения → путь».
fn walk(dir: &Path, depth: usize, map: &mut HashMap<String, PathBuf>, count: &mut usize) {
    if depth == 0 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let path = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            // Скрытые каталоги и мусор пропускаем, но подпапки списков обходим.
            let name = e.file_name().to_string_lossy().to_string();
            if !name.starts_with('.') {
                walk(&path, depth - 1, map, count);
            }
            continue;
        }
        if !is_audio(&path) {
            continue;
        }
        if e.metadata().map(|m| m.len() == 0).unwrap_or(true) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        *count += 1;
        map.entry(key(stem)).or_insert(path);
    }
}

/// Сколько аудиофайлов реально лежит в каталоге, включая подпапки списков.
pub fn count_audio(dir: &Path) -> usize {
    let mut map = HashMap::new();
    let mut count = 0;
    walk(dir, MAX_DEPTH, &mut map, &mut count);
    count
}

/// Индекс «имя файла без расширения → путь» по всем каталогам поиска.
fn index() -> HashMap<String, PathBuf> {
    if let Some(i) = INDEX.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        return i.clone();
    }
    let mut map = HashMap::new();
    let mut count = 0;
    for dir in dirs() {
        walk(&dir, MAX_DEPTH, &mut map, &mut count);
    }
    *INDEX.lock().unwrap_or_else(|e| e.into_inner()) = Some(map.clone());
    map
}

/// Забыть индекс: вызывать после скачивания, смены папки или удаления файлов.
pub fn refresh() {
    *INDEX.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Готовый файл трека в указанном каталоге: не `.part`, ненулевого размера.
pub fn find_in(dir: &Path, t: &Track) -> Option<PathBuf> {
    let stem = track_file_stem(t);
    EXTS.iter()
        .map(|e| dir.join(format!("{stem}.{e}")))
        .find(|p| std::fs::metadata(p).is_ok_and(|m| m.len() > 0))
}

/// Трек лежит где-то внутри каталога, включая подпапки списков.
fn find_under(dir: &Path, t: &Track) -> Option<PathBuf> {
    let mut map = HashMap::new();
    let mut count = 0;
    walk(dir, MAX_DEPTH, &mut map, &mut count);
    map.get(&key(&track_file_stem(t))).cloned()
}

/// Трек уже скачан в этот каталог или в его подпапки.
pub fn saved_already(dir: &Path, t: &Track) -> bool {
    find_in(dir, t).is_some() || find_under(dir, t).is_some()
}

/// Путь к скачанному треку (по всем каталогам поиска).
pub fn find(t: &Track) -> Option<PathBuf> {
    let stem = track_file_stem(t);
    // Сначала точное имя в каталоге скачивания — это самый частый и быстрый случай.
    if let Some(first) = dirs().first() {
        if let Some(p) = find_in(first, t) {
            return Some(p);
        }
    }
    let idx = index();
    if let Some(p) = idx.get(&key(&stem)) {
        return Some(p.clone());
    }
    // Файл могли сохранить с другим регистром/пробелами — пробуем ещё раз по префиксу.
    let k = key(&stem);
    idx.iter().find(|(name, _)| name.starts_with(&k) || k.starts_with(name.as_str())).map(|(_, p)| p.clone())
}

/// Есть ли трек на диске.
pub fn is_local(t: &Track) -> bool {
    find(t).is_some()
}

/// Прочитать трек с диска, если он скачан.
pub fn read(t: &Track) -> Option<(PathBuf, Vec<u8>)> {
    let path = find(t)?;
    let bytes = std::fs::read(&path).ok()?;
    if bytes.is_empty() {
        return None;
    }
    Some((path, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(title: &str, artists: &str) -> Track {
        Track {
            id: "57703".into(),
            album_id: None,
            title: title.into(),
            version: String::new(),
            artists: artists.into(),
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
        let base = std::env::temp_dir().join(format!("yamusic-local-test-{}", std::process::id()));
        let dir = base.join("Яндекс Музыка");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&dir).unwrap();
        set_dirs(vec![dir.clone()]);
        let t = track("Трек", "Артист");
        assert!(!is_local(&t), "до скачивания файла на диске нет");
        let name = format!("{}.mp3", track_file_stem(&t));
        std::fs::write(dir.join(&name), b"ID3test-bytes").unwrap();
        refresh();
        assert!(is_local(&t), "скачанный файл {name} не найден");
        let (p, b) = read(&t).expect("скачанный файл читается");
        assert_eq!(p.file_name().unwrap().to_string_lossy(), name);
        assert_eq!(b, b"ID3test-bytes");
        // Другая папка в списке поиска тоже годится.
        let other_dir = base.join("Музыка").join("Яндекс Музыка");
        std::fs::create_dir_all(&other_dir).unwrap();
        let t2 = track("Второй", "Артист");
        std::fs::write(other_dir.join(format!("{}.m4a", track_file_stem(&t2))), b"xx").unwrap();
        set_dirs(vec![dir.clone(), other_dir]);
        refresh();
        assert!(is_local(&t2), "файл из второго каталога не найден");
        // Пустой файл и обрывок .part скачанными не считаются.
        let t3 = track("Третий", "Артист");
        std::fs::write(dir.join(format!("{}.mp3", track_file_stem(&t3))), b"").unwrap();
        std::fs::write(dir.join(format!("{}.part", track_file_stem(&t3))), b"x").unwrap();
        refresh();
        assert!(!is_local(&t3), "пустой файл не считается скачанным");
        // Список качается в подпапку («Мне нравится», название плейлиста) — трек оттуда
        // тоже должен находиться, иначе нет ни галочки, ни игры с диска.
        let sub = dir.join("Мне нравится");
        std::fs::create_dir_all(&sub).unwrap();
        let t4 = track("Четвёртый", "Артист");
        std::fs::write(sub.join(format!("{}.mp3", track_file_stem(&t4))), b"ID3sub").unwrap();
        refresh();
        assert!(is_local(&t4), "трек из подпапки списка не найден");
        assert_eq!(count_audio(&dir), 2, "в подпапке списка один файл, плюс один в корне");
        assert!(saved_already(&dir, &t4), "скачанное в подпапке не перекачивается");
        let _ = std::fs::remove_dir_all(&base);
    }
}
