//! Потоковое воспроизведение: байты трека приходят по сети и сразу читаются декодером.
//!
//! Раньше трек целиком скачивался в память, и до последнего байта звука не было.
//! Здесь загрузчик дописывает куски в общий буфер, а декодер читает его как обычный файл:
//! чтение за позицией ждёт следующего куска, конец файла — только когда загрузка закончилась.

use std::io::{self, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

/// Общий буфер: загрузчик дописывает, декодер вычитывает.
#[derive(Default)]
pub struct Shared {
    data: Mutex<Vec<u8>>,
    cv: Condvar,
    done: AtomicBool,
    err: Mutex<Option<String>>,
}

impl Shared {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Дописать пришедший по сети кусок.
    pub fn push(&self, chunk: &[u8]) {
        {
            let mut d = self.data.lock().unwrap_or_else(|e| e.into_inner());
            d.extend_from_slice(chunk);
        }
        self.cv.notify_all();
    }

    /// Загрузка завершена: читатель получит конец файла.
    pub fn finish(&self) {
        self.done.store(true, Ordering::SeqCst);
        self.cv.notify_all();
    }

    pub fn fail(&self, e: String) {
        *self.err.lock().unwrap_or_else(|e| e.into_inner()) = Some(e);
        self.finish();
    }

    pub fn err(&self) -> Option<String> {
        self.err.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn len(&self) -> usize {
        self.data.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::SeqCst)
    }

    /// Дождаться, пока в буфере наберётся `want` байт (или загрузка закончится).
    pub fn wait_len(&self, want: usize) {
        let mut d = self.data.lock().unwrap_or_else(|e| e.into_inner());
        while d.len() < want && !self.is_done() {
            d = self.cv.wait(d).unwrap_or_else(|e| e.into_inner());
        }
    }
}

/// Читатель с Seek: для декодера это обычный файл, байты подвозятся по сети.
pub struct Reader {
    shared: Arc<Shared>,
    pos: u64,
    total: Option<u64>,
}

impl Reader {
    /// `total` — длина файла из ответа сервера, если он её сообщил.
    pub fn new(shared: Arc<Shared>, total: Option<u64>) -> Self {
        Self { shared, pos: 0, total }
    }
}

impl Read for Reader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let mut d = self.shared.data.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            let have = d.len();
            if (self.pos as usize) < have {
                let start = self.pos as usize;
                let n = out.len().min(have - start);
                out[..n].copy_from_slice(&d[start..start + n]);
                self.pos += n as u64;
                return Ok(n);
            }
            if self.shared.is_done() {
                return match self.shared.err() {
                    Some(e) => Err(io::Error::new(io::ErrorKind::Other, e)),
                    None => Ok(0),
                };
            }
            d = self.shared.cv.wait(d).unwrap_or_else(|e| e.into_inner());
        }
    }
}

impl Seek for Reader {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let next: i64 = match from {
            SeekFrom::Start(p) => p as i64,
            SeekFrom::Current(o) => self.pos as i64 + o,
            SeekFrom::End(o) => match self.total {
                Some(t) => t as i64 + o,
                // Длина заранее неизвестна: дожидаемся конца загрузки.
                None => {
                    let mut d = self.shared.data.lock().unwrap_or_else(|e| e.into_inner());
                    while !self.shared.is_done() {
                        d = self.shared.cv.wait(d).unwrap_or_else(|e| e.into_inner());
                    }
                    d.len() as i64 + o
                }
            },
        };
        if next < 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "позиция до начала файла"));
        }
        self.pos = next as u64;
        Ok(self.pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn читатель_отдаёт_байты_по_мере_прихода() {
        let sh = Shared::new();
        let mut r = Reader::new(sh.clone(), Some(8));
        sh.push(b"abcd");
        let mut out = [0u8; 4];
        assert_eq!(r.read(&mut out).unwrap(), 4);
        assert_eq!(&out, b"abcd");
        // Данных пока нет — дочитываем после прихода следующего куска.
        std::thread::spawn({
            let sh = sh.clone();
            move || {
                std::thread::sleep(std::time::Duration::from_millis(50));
                sh.push(b"efgh");
                sh.finish();
            }
        });
        let mut out2 = [0u8; 8];
        assert_eq!(r.read(&mut out2).unwrap(), 4);
        assert_eq!(&out2[..4], b"efgh");
        // Конец файла — только после finish.
        assert_eq!(r.read(&mut out2).unwrap(), 0);
    }

    #[test]
    fn поиск_по_известной_длине_и_по_концу() {
        let sh = Shared::new();
        sh.push(b"0123456789");
        sh.finish();
        let mut r = Reader::new(sh.clone(), Some(10));
        assert_eq!(r.seek(SeekFrom::Start(2)).unwrap(), 2);
        let mut b = [0u8; 3];
        assert_eq!(r.read(&mut b).unwrap(), 3);
        assert_eq!(&b, b"234");
        assert_eq!(r.seek(SeekFrom::End(-4)).unwrap(), 6);
        let mut c = [0u8; 4];
        assert_eq!(r.read(&mut c).unwrap(), 4);
        assert_eq!(&c, b"6789");
        assert_eq!(r.seek(SeekFrom::Current(-2)).unwrap(), 8);
    }
}
