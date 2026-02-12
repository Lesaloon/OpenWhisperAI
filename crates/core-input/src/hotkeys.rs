use std::{
    collections::{HashMap, HashSet},
    sync::{mpsc, Arc, Mutex},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HotkeyKey {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    Space,
    Enter,
    Escape,
    Tab,
    Backspace,
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct HotkeyModifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

impl HotkeyModifiers {
    pub const fn none() -> Self {
        Self {
            ctrl: false,
            alt: false,
            shift: false,
            meta: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hotkey {
    pub key: HotkeyKey,
    pub modifiers: HotkeyModifiers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HotkeyState {
    Pressed,
    Released,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HotkeyTrigger {
    Pressed,
    Released,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HotkeyEvent {
    pub key: HotkeyKey,
    pub modifiers: HotkeyModifiers,
    pub state: HotkeyState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeyBinding {
    pub action: String,
    pub trigger: HotkeyTrigger,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeyActionEvent {
    pub action: String,
    pub hotkey: Hotkey,
    pub state: HotkeyState,
}

#[derive(Debug, thiserror::Error)]
pub enum HotkeyError {
    #[error("hotkey listener error: {0}")]
    Listener(String),
    #[error("hotkey manager lock was poisoned")]
    ManagerLockPoisoned,
}

#[derive(Debug, Default)]
pub struct HotkeyManager {
    bindings: HashMap<Hotkey, HotkeyBinding>,
}

impl HotkeyManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, hotkey: Hotkey, action: impl Into<String>) -> Option<HotkeyBinding> {
        self.register_with_trigger(hotkey, HotkeyTrigger::Pressed, action)
    }

    pub fn register_with_trigger(
        &mut self,
        hotkey: Hotkey,
        trigger: HotkeyTrigger,
        action: impl Into<String>,
    ) -> Option<HotkeyBinding> {
        self.bindings.insert(
            hotkey,
            HotkeyBinding {
                action: action.into(),
                trigger,
            },
        )
    }

    pub fn unregister(&mut self, hotkey: &Hotkey) -> Option<HotkeyBinding> {
        self.bindings.remove(hotkey)
    }

    pub fn resolve(&self, event: &HotkeyEvent) -> Option<&str> {
        let hotkey = Hotkey {
            key: event.key,
            modifiers: event.modifiers,
        };
        self.bindings
            .get(&hotkey)
            .filter(|binding| trigger_matches(binding.trigger, event.state))
            .map(|binding| binding.action.as_str())
    }
}

fn trigger_matches(trigger: HotkeyTrigger, state: HotkeyState) -> bool {
    matches!(
        (trigger, state),
        (HotkeyTrigger::Pressed, HotkeyState::Pressed)
            | (HotkeyTrigger::Released, HotkeyState::Released)
    )
}

pub struct HotkeyListenerHandle {
    join_handle: std::thread::JoinHandle<Result<(), HotkeyError>>,
}

impl HotkeyListenerHandle {
    pub fn join(self) -> Result<(), HotkeyError> {
        match self.join_handle.join() {
            Ok(result) => result,
            Err(_) => Err(HotkeyError::Listener(
                "listener thread panicked".to_string(),
            )),
        }
    }
}

pub struct GlobalHotkeyListener {
    manager: Arc<Mutex<HotkeyManager>>,
}

impl GlobalHotkeyListener {
    pub fn new(manager: Arc<Mutex<HotkeyManager>>) -> Self {
        Self { manager }
    }

    pub fn start(
        &self,
    ) -> Result<(HotkeyListenerHandle, mpsc::Receiver<HotkeyActionEvent>), HotkeyError> {
        let (sender, receiver) = mpsc::channel();
        let manager = Arc::clone(&self.manager);

        let handle = spawn_listener(manager, sender, |mut handler| {
            rdev::listen(move |event| handler(event))
                .map_err(|error| HotkeyError::Listener(format!("{error:?}")))
        });

        Ok((handle, receiver))
    }
}

fn spawn_listener(
    manager: Arc<Mutex<HotkeyManager>>,
    sender: mpsc::Sender<HotkeyActionEvent>,
    listen: impl FnOnce(Box<dyn FnMut(rdev::Event) + Send>) -> Result<(), HotkeyError> + Send + 'static,
) -> HotkeyListenerHandle {
    let join_handle = std::thread::spawn(move || {
        let mut modifiers = ModifierState::default();
        let mut pressed_keys = HashSet::new();
        let mut handler = move |event: rdev::Event| match event.event_type {
            rdev::EventType::KeyPress(key) => {
                if modifiers.update(key, true) {
                    return;
                }

                if let Some(mapped) = map_key(key) {
                    if !pressed_keys.insert(mapped) {
                        return;
                    }

                    let event = HotkeyEvent {
                        key: mapped,
                        modifiers: modifiers.as_modifiers(),
                        state: HotkeyState::Pressed,
                    };

                    if let Ok(manager) = manager.lock() {
                        if let Some(action) = manager.resolve(&event) {
                            let _ = sender.send(HotkeyActionEvent {
                                action: action.to_string(),
                                hotkey: Hotkey {
                                    key: event.key,
                                    modifiers: event.modifiers,
                                },
                                state: event.state,
                            });
                        }
                    }
                }
            }
            rdev::EventType::KeyRelease(key) => {
                modifiers.update(key, false);

                if let Some(mapped) = map_key(key) {
                    if !pressed_keys.remove(&mapped) {
                        return;
                    }

                    let event = HotkeyEvent {
                        key: mapped,
                        modifiers: modifiers.as_modifiers(),
                        state: HotkeyState::Released,
                    };

                    if let Ok(manager) = manager.lock() {
                        if let Some(action) = manager.resolve(&event) {
                            let _ = sender.send(HotkeyActionEvent {
                                action: action.to_string(),
                                hotkey: Hotkey {
                                    key: event.key,
                                    modifiers: event.modifiers,
                                },
                                state: event.state,
                            });
                        }
                    }
                }
            }
            _ => {}
        };

        listen(Box::new(move |event| handler(event)))
    });

    HotkeyListenerHandle { join_handle }
}

#[derive(Default)]
struct ModifierState {
    ctrl: bool,
    alt: bool,
    shift: bool,
    meta: bool,
}

impl ModifierState {
    fn update(&mut self, key: rdev::Key, pressed: bool) -> bool {
        match key {
            rdev::Key::ControlLeft | rdev::Key::ControlRight => {
                self.ctrl = pressed;
                true
            }
            rdev::Key::ShiftLeft | rdev::Key::ShiftRight => {
                self.shift = pressed;
                true
            }
            rdev::Key::Alt | rdev::Key::AltGr => {
                self.alt = pressed;
                true
            }
            rdev::Key::MetaLeft | rdev::Key::MetaRight => {
                self.meta = pressed;
                true
            }
            _ => false,
        }
    }

    fn as_modifiers(&self) -> HotkeyModifiers {
        HotkeyModifiers {
            ctrl: self.ctrl,
            alt: self.alt,
            shift: self.shift,
            meta: self.meta,
        }
    }
}

fn map_key(key: rdev::Key) -> Option<HotkeyKey> {
    match key {
        rdev::Key::KeyA => Some(HotkeyKey::A),
        rdev::Key::KeyB => Some(HotkeyKey::B),
        rdev::Key::KeyC => Some(HotkeyKey::C),
        rdev::Key::KeyD => Some(HotkeyKey::D),
        rdev::Key::KeyE => Some(HotkeyKey::E),
        rdev::Key::KeyF => Some(HotkeyKey::F),
        rdev::Key::KeyG => Some(HotkeyKey::G),
        rdev::Key::KeyH => Some(HotkeyKey::H),
        rdev::Key::KeyI => Some(HotkeyKey::I),
        rdev::Key::KeyJ => Some(HotkeyKey::J),
        rdev::Key::KeyK => Some(HotkeyKey::K),
        rdev::Key::KeyL => Some(HotkeyKey::L),
        rdev::Key::KeyM => Some(HotkeyKey::M),
        rdev::Key::KeyN => Some(HotkeyKey::N),
        rdev::Key::KeyO => Some(HotkeyKey::O),
        rdev::Key::KeyP => Some(HotkeyKey::P),
        rdev::Key::KeyQ => Some(HotkeyKey::Q),
        rdev::Key::KeyR => Some(HotkeyKey::R),
        rdev::Key::KeyS => Some(HotkeyKey::S),
        rdev::Key::KeyT => Some(HotkeyKey::T),
        rdev::Key::KeyU => Some(HotkeyKey::U),
        rdev::Key::KeyV => Some(HotkeyKey::V),
        rdev::Key::KeyW => Some(HotkeyKey::W),
        rdev::Key::KeyX => Some(HotkeyKey::X),
        rdev::Key::KeyY => Some(HotkeyKey::Y),
        rdev::Key::KeyZ => Some(HotkeyKey::Z),
        rdev::Key::F1 => Some(HotkeyKey::F1),
        rdev::Key::F2 => Some(HotkeyKey::F2),
        rdev::Key::F3 => Some(HotkeyKey::F3),
        rdev::Key::F4 => Some(HotkeyKey::F4),
        rdev::Key::F5 => Some(HotkeyKey::F5),
        rdev::Key::F6 => Some(HotkeyKey::F6),
        rdev::Key::F7 => Some(HotkeyKey::F7),
        rdev::Key::F8 => Some(HotkeyKey::F8),
        rdev::Key::F9 => Some(HotkeyKey::F9),
        rdev::Key::F10 => Some(HotkeyKey::F10),
        rdev::Key::F11 => Some(HotkeyKey::F11),
        rdev::Key::F12 => Some(HotkeyKey::F12),
        rdev::Key::Space => Some(HotkeyKey::Space),
        rdev::Key::Return => Some(HotkeyKey::Enter),
        rdev::Key::Escape => Some(HotkeyKey::Escape),
        rdev::Key::Tab => Some(HotkeyKey::Tab),
        rdev::Key::Backspace => Some(HotkeyKey::Backspace),
        rdev::Key::LeftArrow => Some(HotkeyKey::Left),
        rdev::Key::RightArrow => Some(HotkeyKey::Right),
        rdev::Key::UpArrow => Some(HotkeyKey::Up),
        rdev::Key::DownArrow => Some(HotkeyKey::Down),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        spawn_listener, Hotkey, HotkeyError, HotkeyEvent, HotkeyKey, HotkeyManager,
        HotkeyModifiers, HotkeyState, HotkeyTrigger,
    };
    use std::sync::{mpsc, Arc, Mutex};
    use std::time::SystemTime;

    #[test]
    fn hotkey_manager_resolves_event() {
        let mut manager = HotkeyManager::new();
        let hotkey = Hotkey {
            key: HotkeyKey::F9,
            modifiers: HotkeyModifiers {
                ctrl: true,
                alt: false,
                shift: false,
                meta: false,
            },
        };

        manager.register(hotkey, "toggle-capture");

        let event = HotkeyEvent {
            key: HotkeyKey::F9,
            modifiers: HotkeyModifiers {
                ctrl: true,
                alt: false,
                shift: false,
                meta: false,
            },
            state: HotkeyState::Pressed,
        };

        assert_eq!(manager.resolve(&event), Some("toggle-capture"));
    }

    #[test]
    fn hotkey_manager_requires_exact_modifiers() {
        let mut manager = HotkeyManager::new();
        let hotkey = Hotkey {
            key: HotkeyKey::F9,
            modifiers: HotkeyModifiers {
                ctrl: true,
                alt: false,
                shift: false,
                meta: false,
            },
        };

        manager.register(hotkey, "toggle-capture");

        let event = HotkeyEvent {
            key: HotkeyKey::F9,
            modifiers: HotkeyModifiers {
                ctrl: false,
                alt: false,
                shift: false,
                meta: false,
            },
            state: HotkeyState::Pressed,
        };

        assert_eq!(manager.resolve(&event), None);
    }

    #[test]
    fn hotkey_manager_respects_trigger_type() {
        let mut manager = HotkeyManager::new();
        let hotkey = Hotkey {
            key: HotkeyKey::F10,
            modifiers: HotkeyModifiers::none(),
        };

        manager.register_with_trigger(hotkey, HotkeyTrigger::Released, "release-only");

        let pressed_event = HotkeyEvent {
            key: HotkeyKey::F10,
            modifiers: HotkeyModifiers::none(),
            state: HotkeyState::Pressed,
        };
        let released_event = HotkeyEvent {
            key: HotkeyKey::F10,
            modifiers: HotkeyModifiers::none(),
            state: HotkeyState::Released,
        };

        assert_eq!(manager.resolve(&pressed_event), None);
        assert_eq!(manager.resolve(&released_event), Some("release-only"));
    }

    #[test]
    fn hotkey_listener_propagates_listen_error() {
        let manager = Arc::new(Mutex::new(HotkeyManager::new()));
        let (sender, _receiver) = mpsc::channel();
        let handle = spawn_listener(manager, sender, |_handler| {
            Err(HotkeyError::Listener("listen failed".to_string()))
        });

        let result = handle.join();

        assert!(
            matches!(result, Err(HotkeyError::Listener(message)) if message == "listen failed")
        );
    }

    #[test]
    fn hotkey_listener_ignores_repeat_presses() {
        let manager = Arc::new(Mutex::new(HotkeyManager::new()));
        let hotkey = Hotkey {
            key: HotkeyKey::F9,
            modifiers: HotkeyModifiers::none(),
        };
        manager
            .lock()
            .expect("manager")
            .register(hotkey, "toggle-capture");

        let (sender, receiver) = mpsc::channel();
        let handle = spawn_listener(manager, sender, |mut handler| {
            let press = rdev::Event {
                time: SystemTime::now(),
                name: None,
                event_type: rdev::EventType::KeyPress(rdev::Key::F9),
            };
            handler(press.clone());
            handler(press);

            let release = rdev::Event {
                time: SystemTime::now(),
                name: None,
                event_type: rdev::EventType::KeyRelease(rdev::Key::F9),
            };
            handler(release);
            Ok(())
        });

        handle.join().expect("listener join");

        let result = receiver.try_iter().collect::<Vec<_>>();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].action, "toggle-capture");
        assert_eq!(result[0].state, HotkeyState::Pressed);
    }
}
