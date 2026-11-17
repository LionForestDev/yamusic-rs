//! MPRIS (org.mpris.MediaPlayer2) — стандартный способ, которым рабочий стол управляет
//! плеером: медиаклавиши клавиатуры, кнопки на панели KDE, playerctl.
//!
//! Своей логики у интерфейса нет: он спрашивает состояние у окна (поток UI) и передаёт
//! ему команды. Так MPRIS ничего не ломает, если окно занято.

use std::collections::HashMap;
use std::sync::mpsc;
use std::time::Duration;

use zbus::blocking::Connection;
use zbus::interface;
use zbus::zvariant::{ObjectPath, OwnedValue, Value};

/// Путь объекта, который ждут все клиенты MPRIS.
const PATH: &str = "/org/mpris/MediaPlayer2";
/// Имя плеера в шине: по нему KDE и playerctl находят приложение.
const NAME: &str = "org.mpris.MediaPlayer2.yamusic-rs";

/// Снимок состояния плеера для MPRIS.
pub struct Info {
    pub playing: bool,
    /// Трек был выбран (плеер не пустой), но сейчас остановлен.
    pub stopped: bool,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub length_us: i64,
    pub position_us: i64,
    pub track_id: String,
    pub art_url: String,
    pub volume: f64,
    pub can_prev: bool,
    pub can_next: bool,
}

/// Спросить состояние у окна: интерфейс живёт в потоке UI, поэтому запрос туда и обратно.
fn on_ui<R: Send + 'static>(f: impl FnOnce() -> R + Send + 'static) -> Option<R> {
    let (tx, rx) = mpsc::channel();
    slint::invoke_from_event_loop(move || {
        let _ = tx.send(f());
    })
    .ok()?;
    rx.recv_timeout(Duration::from_secs(2)).ok()
}

fn info() -> Info {
    on_ui(crate::gui::mpris_info).unwrap_or(Info {
        playing: false,
        stopped: true,
        title: String::new(),
        artist: String::new(),
        album: String::new(),
        length_us: 0,
        position_us: 0,
        track_id: String::new(),
        art_url: String::new(),
        volume: 0.0,
        can_prev: false,
        can_next: false,
    })
}

/// Команду выполняем в потоке UI, ответ не ждём: интерфейс должен отвечать быстро.
fn command(f: fn()) {
    let _ = slint::invoke_from_event_loop(f);
}

fn track_path(id: &str) -> ObjectPath<'static> {
    let clean: String = id.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    ObjectPath::try_from(format!("/org/yamusic/rs/track/{clean}")).unwrap_or_else(|_| ObjectPath::try_from("/org/yamusic/rs/track/0").unwrap())
}

fn val<T: Into<Value<'static>>>(v: T) -> OwnedValue {
    OwnedValue::try_from(v.into()).expect("простое значение MPRIS")
}

/// Состояние в виде, удобном для сравнения: что изменилось с прошлого раза.
#[derive(Clone, PartialEq)]
struct Snapshot {
    status: String,
    meta_key: String,
    volume_bits: u64,
    can_next: bool,
    can_prev: bool,
}

fn status_of(i: &Info) -> String {
    if i.playing {
        "Playing".into()
    } else if i.stopped {
        "Stopped".into()
    } else {
        "Paused".into()
    }
}

fn snapshot(i: &Info) -> Snapshot {
    Snapshot {
        status: status_of(i),
        meta_key: format!(
            "{}|{}|{}|{}|{}|{}",
            i.track_id, i.title, i.artist, i.album, i.length_us, i.art_url
        ),
        volume_bits: i.volume.to_bits(),
        can_next: i.can_next,
        can_prev: i.can_prev,
    }
}

fn metadata_of(i: &Info) -> HashMap<String, OwnedValue> {
    let mut m: HashMap<String, OwnedValue> = HashMap::new();
    m.insert("mpris:trackid".into(), val(track_path(&i.track_id)));
    if !i.title.is_empty() {
        m.insert("xesam:title".into(), val(i.title.clone()));
        m.insert("xesam:artist".into(), val(vec![i.artist.clone()]));
        m.insert("xesam:album".into(), val(i.album.clone()));
    }
    if i.length_us > 0 {
        m.insert("mpris:length".into(), val(i.length_us));
    }
    if !i.art_url.is_empty() {
        m.insert("mpris:artUrl".into(), val(i.art_url.clone()));
    }
    m
}

/// Разослать PropertiesChanged по изменившимся свойствам. Без этих сигналов рабочий стол
/// (KDE, GNOME) не понимает, что плеер заиграл: медиаклавиши уходят другому плееру или
/// в пустоту, а панель показывает пустой виджет.
fn emit_changes(conn: &Connection, now: &Info, prev: &Snapshot) -> zbus::Result<()> {
    let cur = snapshot(now);
    let mut changed: HashMap<String, OwnedValue> = HashMap::new();
    if cur.status != prev.status {
        changed.insert("PlaybackStatus".into(), val(cur.status.clone()));
    }
    if cur.meta_key != prev.meta_key {
        changed.insert("Metadata".into(), val(metadata_of(now)));
    }
    if cur.volume_bits != prev.volume_bits {
        changed.insert("Volume".into(), val(now.volume));
    }
    if cur.can_next != prev.can_next {
        changed.insert("CanGoNext".into(), val(cur.can_next));
    }
    if cur.can_prev != prev.can_prev {
        changed.insert("CanGoPrevious".into(), val(cur.can_prev));
    }
    if changed.is_empty() {
        return Ok(());
    }
    let body = ("org.mpris.MediaPlayer2.Player".to_string(), changed, Vec::<String>::new());
    conn.emit_signal(None::<&str>, PATH, "org.freedesktop.DBus.Properties", "PropertiesChanged", &body)
}

/// Корневой интерфейс: имя приложения, «поднять окно», «выйти».
struct Root;

#[interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    #[zbus(property)]
    fn identity(&self) -> String {
        "Яндекс Музыка RS".into()
    }

    #[zbus(property)]
    fn desktop_entry(&self) -> String {
        "yamusic-rs".into()
    }

    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        Vec::new()
    }

    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        Vec::new()
    }

    #[zbus(property)]
    fn can_quit(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_raise(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }

    fn raise(&self) {
        command(crate::gui::mpris_raise);
    }

    fn quit(&self) {
        command(crate::gui::mpris_quit);
    }
}

/// Плеер: состояние и управление.
struct Player;

#[interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    fn play(&self) {
        crate::config::log("MPRIS: команда Play");
        command(crate::gui::mpris_play);
    }

    fn pause(&self) {
        crate::config::log("MPRIS: команда Pause");
        command(crate::gui::mpris_pause);
    }

    fn play_pause(&self) {
        crate::config::log("MPRIS: команда PlayPause");
        command(crate::gui::mpris_play_pause);
    }

    fn stop(&self) {
        command(crate::gui::mpris_stop);
    }

    fn next(&self) {
        crate::config::log("MPRIS: команда Next");
        command(crate::gui::mpris_next);
    }

    fn previous(&self) {
        crate::config::log("MPRIS: команда Previous");
        command(crate::gui::mpris_prev);
    }

    #[zbus(property)]
    fn playback_status(&self) -> String {
        status_of(&info())
    }

    #[zbus(property)]
    fn loop_status(&self) -> String {
        "None".into()
    }

    #[zbus(property)]
    fn set_loop_status(&self, _value: String) {}

    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn set_rate(&self, _value: f64) {}

    #[zbus(property)]
    fn shuffle(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn set_shuffle(&self, _value: bool) {}

    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        metadata_of(&info())
    }

    #[zbus(property)]
    fn volume(&self) -> f64 {
        info().volume
    }

    #[zbus(property)]
    fn set_volume(&self, value: f64) {
        let v = value.clamp(0.0, 1.0) as f32;
        let _ = slint::invoke_from_event_loop(move || crate::gui::mpris_set_volume(v));
    }

    #[zbus(property)]
    fn position(&self) -> i64 {
        info().position_us
    }

    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        info().can_next
    }

    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        info().can_prev
    }

    #[zbus(property)]
    fn can_play(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_seek(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
}

/// Поднять шину и зарегистрировать оба интерфейса. Запускается один раз при старте окна.
pub fn start() {
    std::thread::Builder::new()
        .name("mpris".into())
        .spawn(|| {
            let conn = match Connection::session() {
                Ok(c) => c,
                Err(e) => {
                    crate::config::log(format!("MPRIS: нет сессионной шины D-Bus: {e}"));
                    return;
                }
            };
            if let Err(e) = conn.request_name(NAME) {
                crate::config::log(format!("MPRIS: имя {NAME} не занято: {e}"));
                return;
            }
            let server = conn.object_server();
            if let Err(e) = server.at(PATH, Root) {
                crate::config::log(format!("MPRIS: корневой интерфейс не зарегистрирован: {e}"));
                return;
            }
            if let Err(e) = server.at(PATH, Player) {
                crate::config::log(format!("MPRIS: интерфейс плеера не зарегистрирован: {e}"));
                return;
            }
            crate::config::log("MPRIS: плеер доступен рабочим столу (медиаклавиши, панель)");
            // Опрашиваем состояние и рассылаем PropertiesChanged: по этим сигналам рабочий
            // стол понимает, что играет именно этот плеер, и направляет сюда медиаклавиши.
            let watch = conn.clone();
            std::thread::Builder::new()
                .name("mpris-signals".into())
                .spawn(move || {
                    let mut prev: Option<Snapshot> = None;
                    loop {
                        std::thread::sleep(Duration::from_millis(400));
                        let now = info();
                        if let Some(p) = &prev {
                            if let Err(e) = emit_changes(&watch, &now, p) {
                                crate::config::log(format!("MPRIS: сигнал об изменении не отправлен: {e}"));
                            }
                        }
                        prev = Some(snapshot(&now));
                    }
                })
                .ok();
            // Поток живёт, пока живёт приложение: шина закрывается вместе с процессом.
            loop {
                std::thread::sleep(Duration::from_secs(3600));
            }
        })
        .ok();
}
