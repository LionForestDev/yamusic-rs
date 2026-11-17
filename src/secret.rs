//! Шифрование токена средствами Windows (DPAPI): расшифровать его может только
//! та же учётная запись на том же компьютере. В config.json лежит base64 шифротекста.

use base64::Engine as _;

/// Дополнительная «соль» DPAPI: другие программы того же пользователя без неё не расшифруют.
/// Старое имя оставлено намеренно: с другой солью сохранённые токены перестанут читаться.
#[cfg_attr(not(windows), allow(dead_code))]
const ENTROPY: &[u8] = b"yamusic-lite token v1";

#[cfg(windows)]
mod dpapi {
    use super::ENTROPY;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    fn blob(b: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB { cbData: b.len() as u32, pbData: b.as_ptr() as *mut u8 }
    }

    fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        let v = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
        unsafe { LocalFree(out.pbData as _) };
        v
    }

    pub fn protect(data: &[u8]) -> Option<Vec<u8>> {
        let (input, entropy) = (blob(data), blob(ENTROPY));
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        let ok = unsafe {
            CryptProtectData(&input, std::ptr::null(), &entropy, std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        };
        (ok != 0).then(|| take(out))
    }

    pub fn unprotect(data: &[u8]) -> Option<Vec<u8>> {
        let (input, entropy) = (blob(data), blob(ENTROPY));
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        let ok = unsafe {
            CryptUnprotectData(&input, std::ptr::null_mut(), &entropy, std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        };
        (ok != 0).then(|| take(out))
    }
}

/// Зашифровать строку для хранения в конфиге.
pub fn seal(plain: &str) -> Option<String> {
    #[cfg(windows)]
    {
        dpapi::protect(plain.as_bytes()).map(|b| base64::engine::general_purpose::STANDARD.encode(b))
    }
    #[cfg(not(windows))]
    {
        let _ = plain;
        None
    }
}

/// Расшифровать строку из конфига; None — чужая учётная запись или испорченные данные.
pub fn open(sealed: &str) -> Option<String> {
    let raw = base64::engine::general_purpose::STANDARD.decode(sealed.trim()).ok()?;
    #[cfg(windows)]
    {
        String::from_utf8(dpapi::unprotect(&raw)?).ok()
    }
    #[cfg(not(windows))]
    {
        let _ = raw;
        None
    }
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn roundtrip() {
        let s = super::seal("y0_secret").unwrap();
        assert!(!s.contains("y0_secret"));
        assert_eq!(super::open(&s).as_deref(), Some("y0_secret"));
        assert_eq!(super::open("AAAA"), None);
    }
}

/// Связка ключей рабочего стола (Secret Service: GNOME Keyring, KWallet) на Linux.
/// Работаем через secret-tool из libsecret — он есть почти везде, где есть сам рабочий стол.
#[cfg(not(windows))]
pub mod keyring {
    use std::collections::HashMap;
    use std::io::Write as _;
    use std::process::{Command, Stdio};
    use std::sync::Mutex;

    const LABEL: &str = "Яндекс Музыка RS";

    /// Что уже записали в этом запуске: не дёргаем связку на каждое сохранение настроек.
    static LAST: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

    pub fn available() -> bool {
        static OK: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *OK.get_or_init(|| {
            Command::new("secret-tool")
                .arg("--version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        })
    }

    fn same_as_last(kind: &str, value: &str) -> bool {
        let g = LAST.lock().unwrap_or_else(|e| e.into_inner());
        g.as_ref().and_then(|m| m.get(kind)).map(|v| v == value).unwrap_or(false)
    }

    fn remember(kind: &str, value: &str) {
        let mut g = LAST.lock().unwrap_or_else(|e| e.into_inner());
        g.get_or_insert_with(HashMap::new).insert(kind.to_string(), value.to_string());
    }

    /// Положить секрет в связку ключей. false — связки нет или она отказала.
    pub fn save(kind: &str, value: &str) -> bool {
        if value.is_empty() || !available() {
            return false;
        }
        if same_as_last(kind, value) {
            return true;
        }
        let Ok(mut child) = Command::new("secret-tool")
            .args(["store", "--label", LABEL, "service", "yamusic-rs", "kind", kind])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            return false;
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(value.as_bytes());
        }
        let ok = child.wait().map(|s| s.success()).unwrap_or(false);
        if ok {
            remember(kind, value);
        }
        ok
    }

    pub fn load(kind: &str) -> Option<String> {
        if !available() {
            return None;
        }
        let out = Command::new("secret-tool")
            .args(["lookup", "service", "yamusic-rs", "kind", kind])
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let s = String::from_utf8(out.stdout).ok()?.trim().to_string();
        if s.is_empty() {
            return None;
        }
        remember(kind, &s);
        Some(s)
    }

    pub fn clear(kind: &str) {
        let mut g = LAST.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(m) = g.as_mut() {
            m.remove(kind);
        }
        let _ = Command::new("secret-tool")
            .args(["clear", "service", "yamusic-rs", "kind", kind])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}
