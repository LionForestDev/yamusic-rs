//! Linux: значок в лотке через StatusNotifierItem (ksni) — KDE, GNOME с расширением,
//! XFCE, Cinnamon. Медиаклавиши на Linux приходят не сюда, а через MPRIS (src/mpris.rs).
//!
//! ksni работает в своём потоке и не имеет доступа к окну, поэтому: состояние окна
//! складывается в общий снимок из потока UI (`refresh`), а пункты меню просят UI
//! выполнить действие через `deliver` — он вызывается уже в потоке окна.

use std::cell::RefCell;
use std::sync::{Arc, Mutex, OnceLock};

use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::{MenuItem, StandardItem};

/// Действия из меню значка — те же, что на Windows.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Toggle,
    Show,
    PlayPause,
    Prev,
    Next,
    Updates,
    Terms,
    Recommendations,
    Support,
    Quit,
}

/// Что показать в меню: свёрнуто ли окно, играет ли трек, есть ли соседние.
#[allow(dead_code)]
pub struct MenuState {
    pub minimized: bool,
    pub playing: bool,
    pub can_prev: bool,
    pub can_next: bool,
}

impl Default for MenuState {
    fn default() -> Self {
        MenuState { minimized: false, playing: false, can_prev: false, can_next: false }
    }
}

thread_local! {
    /// Обработчик действий (задан окном); вызывать только в потоке UI.
    static HANDLER: RefCell<Option<Box<dyn Fn(Event)>>> = const { RefCell::new(None) };
    /// Как спросить у окна текущее состояние; тоже только в потоке UI.
    static STATE: RefCell<Option<Box<dyn Fn() -> MenuState>>> = const { RefCell::new(None) };
}

/// Снимок состояния для потока значка: обновляется из окна, читается из меню.
#[derive(Clone, Default)]
struct Snapshot {
    playing: bool,
    minimized: bool,
    can_prev: bool,
    can_next: bool,
}

static SNAPSHOT: OnceLock<Arc<Mutex<Snapshot>>> = OnceLock::new();
static HANDLE: OnceLock<Mutex<Option<Handle<Tray>>>> = OnceLock::new();
static AVAILABLE: OnceLock<bool> = OnceLock::new();

fn snapshot() -> &'static Arc<Mutex<Snapshot>> {
    SNAPSHOT.get_or_init(|| Arc::new(Mutex::new(Snapshot::default())))
}

/// Выполнить действие меню в потоке окна.
pub fn deliver(event: Event) {
    HANDLER.with(|h| {
        // Копию замыкания взять нельзя (Fn), поэтому вызываем под заимствованием.
        let borrow = h.borrow();
        if let Some(f) = borrow.as_ref() {
            f(event);
        }
    });
}

/// Пункт меню, который отправляет действие в окно.
fn item(label: &str, event: Event) -> MenuItem<Tray> {
    MenuItem::Standard(StandardItem {
        label: label.into(),
        activate: Box::new(move |_tray: &mut Tray| {
            let _ = slint::invoke_from_event_loop(move || deliver(event));
        }),
        ..Default::default()
    })
}

struct Tray {
    icon: Vec<u8>,
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "yamusic-rs".into()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::ApplicationStatus
    }

    fn title(&self) -> String {
        "Яндекс Музыка RS".into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![ksni::Icon { width: 256, height: 256, data: self.icon.clone() }]
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let s = snapshot().lock().map(|s| s.clone()).unwrap_or_default();
        let state = if s.playing { "играет" } else { "пауза" };
        ksni::ToolTip {
            title: format!("Яндекс Музыка RS — {state}"),
            description: "Нажмите, чтобы показать окно".into(),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = slint::invoke_from_event_loop(|| deliver(Event::Toggle));
    }

    fn secondary_activate(&mut self, _x: i32, _y: i32) {
        let _ = slint::invoke_from_event_loop(|| deliver(Event::Show));
    }

    fn menu(&self) -> Vec<MenuItem<Tray>> {
        let s = snapshot().lock().map(|s| s.clone()).unwrap_or_default();
        let play_label = if s.playing { "Пауза" } else { "Играть" };
        let mut items = vec![
            item(if s.minimized { "Показать окно" } else { "Свернуть окно" }, Event::Toggle),
            MenuItem::Separator,
            item(play_label, Event::PlayPause),
            item("Предыдущий трек", Event::Prev),
            item("Следующий трек", Event::Next),
            MenuItem::Separator,
            item("Обновления", Event::Updates),
            item("Условия использования", Event::Terms),
            item("Рекомендации", Event::Recommendations),
            item("Поддержка", Event::Support),
            MenuItem::Separator,
            item("Выход", Event::Quit),
        ];
        // Кнопки соседних треков неактивны, пока очередь пуста.
        for it in items.iter_mut() {
            if let MenuItem::Standard(si) = it {
                if si.label.starts_with("Предыдущий") {
                    si.enabled = s.can_prev;
                }
                if si.label.starts_with("Следующий") {
                    si.enabled = s.can_next;
                }
            }
        }
        items
    }
}

/// Значок для лотка: PNG из icons/ → ARGB32, как требует StatusNotifierItem.
fn icon_argb() -> Vec<u8> {
    let png = include_bytes!("../../icons/yamusic-rs-256.png");
    let rgba = match image::load_from_memory_with_format(png, image::ImageFormat::Png) {
        Ok(img) => img.to_rgba8(),
        Err(e) => {
            crate::config::log(format!("значок для лотка не прочитался: {e}"));
            return Vec::new();
        }
    };
    let (w, h) = rgba.dimensions();
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for px in rgba.pixels() {
        let [r, g, b, a] = px.0;
        out.extend_from_slice(&[a, r, g, b]);
    }
    out
}

/// Включить значок в лотке. Возвращает false, если StatusNotifierItem на шине нет.
pub fn install(on_event: impl Fn(Event) + 'static, state: impl Fn() -> MenuState + 'static) {
    HANDLER.with(|h| *h.borrow_mut() = Some(Box::new(on_event)));
    STATE.with(|s| *s.borrow_mut() = Some(Box::new(state)));

    let icon = icon_argb();
    if icon.is_empty() {
        AVAILABLE.set(false).ok();
        return;
    }
    match (Tray { icon }).spawn() {
        Ok(handle) => {
            HANDLE.get_or_init(|| Mutex::new(None)).lock().map(|mut h| *h = Some(handle)).ok();
            AVAILABLE.set(true).ok();
            crate::config::log("трей: значок зарегистрирован (StatusNotifierItem)");
        }
        Err(e) => {
            AVAILABLE.set(false).ok();
            crate::config::log(format!("трей недоступен: {e}"));
        }
    }
}

/// Обновить снимок состояния (вызывается из окна, не чаще раза в тик).
pub fn refresh() {
    let Some(state) = STATE.with(|s| s.borrow().as_ref().map(|f| f())) else { return };
    if let Ok(mut s) = snapshot().lock() {
        s.playing = state.playing;
        s.minimized = state.minimized;
        s.can_prev = state.can_prev;
        s.can_next = state.can_next;
    }
}

/// Есть ли значок в лотке: если нет — окно нельзя прятать, иначе его не вернуть.
pub fn available() -> bool {
    *AVAILABLE.get().unwrap_or(&false)
}

pub fn remove() {
    if let Some(lock) = HANDLE.get() {
        if let Ok(mut h) = lock.lock() {
            if let Some(handle) = h.take() {
                handle.shutdown();
            }
        }
    }
}

/// Второй запуск поднимает окно через сокет единственного экземпляра (см. main.rs).
pub fn wake_existing() -> bool {
    false
}
