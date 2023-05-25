use std::cell::RefCell;
use std::os::fd::AsRawFd;
use std::rc::Rc;
use std::sync::Mutex;

use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::reexports::calloop::{LoopHandle, RegistrationToken};
use smithay_client_toolkit::reexports::client::protocol::wl_keyboard::{
    KeyState as WlKeyState, KeymapFormat, WlKeyboard,
};
use smithay_client_toolkit::reexports::client::protocol::wl_seat::WlSeat;
use smithay_client_toolkit::reexports::client::{Connection, Dispatch, QueueHandle, WEnum};
use std::time::Duration;
use wayland_client::protocol::wl_keyboard;
use wayland_client::Proxy;

use crate::keyboard_types::KeyState;
use crate::Modifiers;
use crate::{KeyEvent, WinHandler};

use super::application::Data;
use super::seat::GlazierSeatState;
use super::surfaces::buffers;
use super::window::{make_wid, WindowHandle};
use crate::backend::shared::xkb;

pub struct KeyboardState {
    /// The underlying WlKeyboard.
    pub keyboard: WlKeyboard,

    /// Loop handle to handle key repeat.
    pub loop_handle: LoopHandle<'static, Data>,

    /// The state of the keyboard.
    xkb_context: xkb::Context,
    xkb_keymap: std::cell::RefCell<Option<xkb::Keymap>>,
    xkb_state: std::cell::RefCell<Option<xkb::State>>,
    xkb_mods: std::cell::Cell<Modifiers>,

    /// The information about the repeat rate obtained from the compositor.
    pub repeat_info: RepeatInfo,

    /// The token of the current handle inside the calloop's event loop.
    pub repeat_token: Option<RegistrationToken>,

    /// The current repeat raw key.
    pub current_repeat: Option<u32>,
}

impl KeyboardState {
    pub fn new(keyboard: WlKeyboard, loop_handle: LoopHandle<'static, Data>) -> Self {
        Self {
            keyboard,
            loop_handle,
            xkb_context: xkb::Context::new(),
            xkb_keymap: std::cell::RefCell::new(None),
            xkb_state: std::cell::RefCell::new(None),
            xkb_mods: std::cell::Cell::new(Modifiers::empty()),
            repeat_info: RepeatInfo::default(),
            repeat_token: None,
            current_repeat: None,
        }
    }
}

/// The rate at which a pressed key is repeated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeatInfo {
    /// Keys will be repeated at the specified rate and delay.
    Repeat {
        /// The time between the key repeats.
        gap: Duration,

        /// Delay (in milliseconds) between a key press and the start of repetition.
        delay: Duration,
    },

    /// Keys should not be repeated.
    Disable,
}

impl Default for RepeatInfo {
    /// The default repeat rate is 25 keys per second with the delay of 200ms.
    ///
    /// The values are picked based on the default in various compositors and Xorg.
    fn default() -> Self {
        Self::Repeat {
            gap: Duration::from_millis(40),
            delay: Duration::from_millis(200),
        }
    }
}

/// The extension to KeyboardData used to store the `window_id`.
pub struct KeyboardData {
    /// The currently focused window surface. Could be `None` on bugged compositors, like mutter.
    pub(super) window_id: Mutex<Option<u64>>,

    /// The seat used to create this keyboard.
    seat: WlSeat,
}

impl KeyboardData {
    pub fn new(seat: WlSeat) -> Self {
        Self {
            window_id: Default::default(),
            seat,
        }
    }
}

impl Dispatch<WlKeyboard, KeyboardData, Data> for Data {
    fn event(
        app_state: &mut Data,
        wl_keyboard: &WlKeyboard,
        event: <WlKeyboard as Proxy>::Event,
        data: &KeyboardData,
        _: &Connection,
        _: &QueueHandle<Data>,
    ) {
        let handles = app_state.handles.clone();
        println!("keyboard event");
        // let window_id = match *data.window_id.lock().unwrap() {
        //     Some(window_id) => window_id,
        //     None => return,
        // };
        // let handler = match app_state
        //     .handles
        //     .borrow()
        //     .get(&window_id)
        //     .and_then(|h| h.handler())
        // {
        //     Some(handler) => handler,
        //     None => return,
        // };

        let seat_state = match app_state.seats.get_mut(&data.seat.id()) {
            Some(seat_state) => seat_state,
            None => return,
        };

        match event {
            wl_keyboard::Event::Keymap { format, fd, size } => {
                let keymap_data = unsafe {
                    buffers::Mmap::from_raw_private(
                        fd.as_raw_fd(),
                        size.try_into().unwrap(),
                        0,
                        size.try_into().unwrap(),
                    )
                    .unwrap()
                    .as_ref()
                    .to_vec()
                };

                match format {
                    WEnum::Value(format) => match format {
                        KeymapFormat::NoKeymap => {
                            tracing::warn!("non-xkb compatible keymap")
                        }
                        KeymapFormat::XkbV1 => unsafe {
                            let keyboard_state = seat_state.keyboard_state.as_mut().unwrap();
                            // keymap data is '\0' terminated.
                            let keymap = keyboard_state.xkb_context.keymap_from_slice(&keymap_data);
                            let keymapstate = keymap.state();

                            keyboard_state.xkb_keymap.replace(Some(keymap));
                            keyboard_state.xkb_state.replace(Some(keymapstate));
                        },
                        _ => {}
                    },
                    WEnum::Unknown(value) => {
                        tracing::warn!("unknown keymap format 0x{:x}", value)
                    }
                }
            }
            wl_keyboard::Event::Enter {
                serial,
                surface,
                keys,
            } => {
                let window_id = make_wid(&surface);

                // Drop the repeat, if there were any.
                seat_state.keyboard_state.as_mut().unwrap().current_repeat = None;

                *data.window_id.lock().unwrap() = Some(window_id);
            }
            wl_keyboard::Event::Leave { serial, surface } => {
                seat_state.keyboard_state.as_mut().unwrap().current_repeat = None;
                *data.window_id.lock().unwrap() = None;
            }
            wl_keyboard::Event::Key {
                serial,
                time,
                key,
                state,
            } if state == WEnum::Value(WlKeyState::Pressed) => {
                println!("key pressed");
                let key = key + 8;

                key_input(
                    handles.clone(),
                    seat_state,
                    data,
                    key,
                    KeyState::Down,
                    false,
                );

                let keyboard_state = seat_state.keyboard_state.as_mut().unwrap();
                let delay = match keyboard_state.repeat_info {
                    RepeatInfo::Repeat { delay, .. } => delay,
                    RepeatInfo::Disable => return,
                };

                keyboard_state.current_repeat = Some(key);

                // NOTE terminate ongoing timer and start a new timer.

                if let Some(token) = keyboard_state.repeat_token.take() {
                    keyboard_state.loop_handle.remove(token);
                }

                let timer = Timer::from_duration(delay);
                let wl_keyboard = wl_keyboard.clone();
                let handles = handles.clone();
                keyboard_state.repeat_token = keyboard_state
                    .loop_handle
                    .insert_source(timer, move |_, _, state| {
                        let data = wl_keyboard.data::<KeyboardData>().unwrap();
                        let seat_state = state.seats.get_mut(&data.seat.id()).unwrap();

                        // NOTE: The removed on event source is batched, but key change to
                        // `None` is instant.
                        let repeat_keycode =
                            match seat_state.keyboard_state.as_ref().unwrap().current_repeat {
                                Some(repeat_keycode) => repeat_keycode,
                                None => return TimeoutAction::Drop,
                            };

                        key_input(
                            handles.clone(),
                            seat_state,
                            data,
                            repeat_keycode,
                            KeyState::Down,
                            true,
                        );

                        // NOTE: the gap could change dynamically while repeat is going.
                        match seat_state.keyboard_state.as_ref().unwrap().repeat_info {
                            RepeatInfo::Repeat { gap, .. } => TimeoutAction::ToDuration(gap),
                            RepeatInfo::Disable => TimeoutAction::Drop,
                        }
                    })
                    .ok();
            }
            wl_keyboard::Event::Key {
                serial,
                time,
                key,
                state,
            } if state == WEnum::Value(WlKeyState::Released) => {
                let keyboard_state = seat_state.keyboard_state.as_mut().unwrap();

                let key = key + 8;

                key_input(handles, seat_state, data, key, KeyState::Up, false);

                let keyboard_state = seat_state.keyboard_state.as_mut().unwrap();
                if keyboard_state.repeat_info != RepeatInfo::Disable {
                    keyboard_state.current_repeat = None;
                }
            }
            wl_keyboard::Event::Modifiers {
                serial,
                mods_depressed,
                mods_latched,
                mods_locked,
                group,
            } => {
                let keyboard_state = seat_state.keyboard_state.as_mut().unwrap();
                keyboard_state.xkb_mods.replace(event_to_mods(event));
            }
            wl_keyboard::Event::RepeatInfo { rate, delay } => {
                let keyboard_state = seat_state.keyboard_state.as_mut().unwrap();
                keyboard_state.repeat_info = if rate == 0 {
                    // Stop the repeat once we get a disable event.
                    keyboard_state.current_repeat = None;
                    if let Some(repeat_token) = keyboard_state.repeat_token.take() {
                        keyboard_state.loop_handle.remove(repeat_token);
                    }
                    RepeatInfo::Disable
                } else {
                    let gap = Duration::from_micros(1_000_000 / rate as u64);
                    let delay = Duration::from_millis(delay as u64);
                    RepeatInfo::Repeat { gap, delay }
                };
            }
            _ => {}
        }
    }
}

fn key_input(
    handles: Rc<RefCell<im::OrdMap<u64, WindowHandle>>>,
    seat_state: &mut GlazierSeatState,
    data: &KeyboardData,
    keycode: u32,
    key_state: KeyState,
    repeat: bool,
) {
    let window_id = match *data.window_id.lock().unwrap() {
        Some(window_id) => window_id,
        None => return,
    };
    let handler = match handles.borrow().get(&window_id).and_then(|h| h.handler()) {
        Some(handler) => handler,
        None => return,
    };

    let keyboard_state = seat_state.keyboard_state.as_mut().unwrap();

    let mut event = keyboard_state
        .xkb_state
        .borrow_mut()
        .as_mut()
        .unwrap()
        .key_event(keycode, key_state, repeat);
    event.mods = keyboard_state.xkb_mods.get();

    match key_state {
        KeyState::Down => {
            handler.borrow_mut().key_down(event);
        }
        KeyState::Up => {
            handler.borrow_mut().key_up(event);
        }
    }
}

// impl KeyboardHandler for Data {
//     fn enter(
//         &mut self,
//         conn: &smithay_client_toolkit::reexports::client::Connection,
//         qh: &smithay_client_toolkit::reexports::client::QueueHandle<Self>,
//         keyboard: &smithay_client_toolkit::reexports::client::protocol::wl_keyboard::WlKeyboard,
//         surface: &smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface,
//         serial: u32,
//         raw: &[u32],
//         keysyms: &[u32],
//     ) {
//         let window_id = make_wid(surface);

//         *keyboard.glazier_data().window_id.lock().unwrap() = Some(window_id);
//     }

//     fn leave(
//         &mut self,
//         conn: &smithay_client_toolkit::reexports::client::Connection,
//         qh: &smithay_client_toolkit::reexports::client::QueueHandle<Self>,
//         keyboard: &smithay_client_toolkit::reexports::client::protocol::wl_keyboard::WlKeyboard,
//         surface: &smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface,
//         serial: u32,
//     ) {
//         let window_id = make_wid(surface);

//         *keyboard.glazier_data().window_id.lock().unwrap() = None;
//     }

//     fn press_key(
//         &mut self,
//         conn: &smithay_client_toolkit::reexports::client::Connection,
//         qh: &smithay_client_toolkit::reexports::client::QueueHandle<Self>,
//         keyboard: &smithay_client_toolkit::reexports::client::protocol::wl_keyboard::WlKeyboard,
//         serial: u32,
//         event: smithay_client_toolkit::seat::keyboard::KeyEvent,
//     ) {
//         self.handle_key_input(keyboard, event, true);
//     }

//     fn release_key(
//         &mut self,
//         conn: &smithay_client_toolkit::reexports::client::Connection,
//         qh: &smithay_client_toolkit::reexports::client::QueueHandle<Self>,
//         keyboard: &smithay_client_toolkit::reexports::client::protocol::wl_keyboard::WlKeyboard,
//         serial: u32,
//         event: smithay_client_toolkit::seat::keyboard::KeyEvent,
//     ) {
//         self.handle_key_input(keyboard, event, false);
//     }

//     fn update_modifiers(
//         &mut self,
//         conn: &smithay_client_toolkit::reexports::client::Connection,
//         qh: &smithay_client_toolkit::reexports::client::QueueHandle<Self>,
//         keyboard: &smithay_client_toolkit::reexports::client::protocol::wl_keyboard::WlKeyboard,
//         serial: u32,
//         mods: smithay_client_toolkit::seat::keyboard::Modifiers,
//     ) {
//         let modifiers = Modifiers::empty();
//         if mods.ctrl {
//             modifiers.set(Modifiers::CONTROL, true);
//         }
//         if mods.alt {
//             modifiers.set(Modifiers::ALT, true);
//         }
//         if mods.shift {
//             modifiers.set(Modifiers::SHIFT, true);
//         }
//         if mods.caps_lock {
//             modifiers.set(Modifiers::CAPS_LOCK, true);
//         }
//         if mods.num_lock {
//             modifiers.set(Modifiers::NUM_LOCK, true);
//         }
//         if mods.logo {
//             modifiers.set(Modifiers::META, true);
//         }

//         let seat_state = self.seats.get_mut(&keyboard.seat().id()).unwrap();
//         seat_state.modifiers = modifiers;

//         // NOTE: part of the workaround from `fn enter`, see it above.
//         let window_id = match *keyboard.glazier_data().window_id.lock().unwrap() {
//             Some(window_id) => window_id,
//             None => {
//                 seat_state.modifiers_pending = true;
//                 return;
//             }
//         };
//     }
// }

#[allow(unused)]
#[derive(Clone)]
struct CachedKeyPress {
    seat: u32,
    serial: u32,
    timestamp: u32,
    key: u32,
    repeat: bool,
    state: wayland_client::protocol::wl_keyboard::KeyState,
    queue: calloop::channel::Sender<KeyEvent>,
}

impl CachedKeyPress {
    fn repeat(&self) -> Self {
        let mut c = self.clone();
        c.repeat = true;
        c
    }
}

#[derive(Debug, Clone)]
struct Repeat {
    rate: std::time::Duration,
    delay: std::time::Duration,
}

impl Default for Repeat {
    fn default() -> Self {
        Self {
            rate: std::time::Duration::from_millis(40),
            delay: std::time::Duration::from_millis(600),
        }
    }
}

struct Keyboard {
    /// Whether we've currently got keyboard focus.
    focused: bool,
    repeat: Repeat,
    last_key_press: Option<CachedKeyPress>,
    xkb_context: xkb::Context,
    xkb_keymap: std::cell::RefCell<Option<xkb::Keymap>>,
    xkb_state: std::cell::RefCell<Option<xkb::State>>,
    xkb_mods: std::cell::Cell<Modifiers>,
}

impl Default for Keyboard {
    fn default() -> Self {
        Self {
            focused: false,
            repeat: Repeat::default(),
            last_key_press: None,
            xkb_context: xkb::Context::new(),
            xkb_keymap: std::cell::RefCell::new(None),
            xkb_state: std::cell::RefCell::new(None),
            xkb_mods: std::cell::Cell::new(Modifiers::empty()),
        }
    }
}

impl Keyboard {
    fn focused(&mut self, updated: bool) {
        self.focused = updated;
    }

    fn repeat(&mut self, u: Repeat) {
        self.repeat = u;
    }

    fn replace_last_key_press(&mut self, u: Option<CachedKeyPress>) {
        self.last_key_press = u;
    }

    fn release_last_key_press(&self, current: &CachedKeyPress) -> Option<CachedKeyPress> {
        match &self.last_key_press {
            None => None, // nothing to do.
            Some(last) => {
                if last.serial >= current.serial {
                    return Some(last.clone());
                }
                if last.key != current.key {
                    return Some(last.clone());
                }
                None
            }
        }
    }

    fn keystroke<'a>(&'a mut self, keystroke: &'a CachedKeyPress) {
        let keystate = match keystroke.state {
            wl_keyboard::KeyState::Released => {
                self.replace_last_key_press(self.release_last_key_press(keystroke));
                KeyState::Up
            }
            wl_keyboard::KeyState::Pressed => {
                self.replace_last_key_press(Some(keystroke.repeat()));
                KeyState::Down
            }
            _ => panic!("unrecognised key event"),
        };

        let mut event = self.xkb_state.borrow_mut().as_mut().unwrap().key_event(
            keystroke.key,
            keystate,
            keystroke.repeat,
        );
        event.mods = self.xkb_mods.get();

        if let Err(cause) = keystroke.queue.send(event) {
            tracing::error!("failed to send druid key event: {:?}", cause);
        }
    }

    fn consume(
        &mut self,
        seat: u32,
        event: wl_keyboard::Event,
        keyqueue: calloop::channel::Sender<KeyEvent>,
    ) {
        tracing::trace!("consume {:?} -> {:?}", seat, event);
        match event {
            wl_keyboard::Event::Keymap { format, fd, size } => {
                // if !matches!(format, wl_keyboard::KeymapFormat::XkbV1) {
                //     panic!("only xkb keymap supported for now");
                // }

                // // TODO to test memory ownership we copy the memory. That way we can deallocate it
                // // and see if we get a segfault.
                // let keymap_data = unsafe {
                //     buffers::Mmap::from_raw_private(
                //         fd,
                //         size.try_into().unwrap(),
                //         0,
                //         size.try_into().unwrap(),
                //     )
                //     .unwrap()
                //     .as_ref()
                //     .to_vec()
                // };

                // // keymap data is '\0' terminated.
                // let keymap = self.xkb_context.keymap_from_slice(&keymap_data);
                // let keymapstate = keymap.state();

                // self.xkb_keymap.replace(Some(keymap));
                // self.xkb_state.replace(Some(keymapstate));
            }
            wl_keyboard::Event::Enter { .. } => {
                self.focused(true);
            }
            wl_keyboard::Event::Leave { .. } => {
                self.focused(false);
            }
            wl_keyboard::Event::Key {
                serial,
                time,
                state,
                key,
            } => {
                // tracing::trace!(
                //     "key stroke registered {:?} {:?} {:?} {:?}",
                //     time,
                //     serial,
                //     key,
                //     state
                // );
                // self.keystroke(&CachedKeyPress {
                //     repeat: false,
                //     seat,
                //     serial,
                //     timestamp: time,
                //     key: key + 8, // TODO: understand the magic 8.
                //     state,
                //     queue: keyqueue,
                // })
            }
            wl_keyboard::Event::Modifiers { .. } => {
                self.xkb_mods.replace(event_to_mods(event));
                println!("modifiers {:?}", self.xkb_mods);
            }
            wl_keyboard::Event::RepeatInfo { rate, delay } => {
                tracing::trace!("keyboard repeat info received {:?} {:?}", rate, delay);
                self.repeat(Repeat {
                    rate: std::time::Duration::from_millis((1000 / rate) as u64),
                    delay: std::time::Duration::from_millis(delay as u64),
                });
            }
            evt => {
                tracing::warn!("unimplemented keyboard event: {:?}", evt);
            }
        }
    }
}

pub(super) struct State {
    apptx: calloop::channel::Sender<KeyEvent>,
    apprx: std::cell::RefCell<Option<calloop::channel::Channel<KeyEvent>>>,
    tx: calloop::channel::Sender<(u32, wl_keyboard::Event, calloop::channel::Sender<KeyEvent>)>,
}

impl Default for State {
    fn default() -> Self {
        let (apptx, apprx) = calloop::channel::channel::<KeyEvent>();
        let (tx, rx) = calloop::channel::channel::<(
            u32,
            wl_keyboard::Event,
            calloop::channel::Sender<KeyEvent>,
        )>();
        let state = Self {
            apptx,
            apprx: std::cell::RefCell::new(Some(apprx)),
            tx,
        };

        std::thread::spawn(move || {
            let mut eventloop: calloop::EventLoop<(calloop::LoopSignal, Keyboard)> =
                calloop::EventLoop::try_new()
                    .expect("failed to initialize the keyboard event loop!");
            let signal = eventloop.get_signal();
            let handle = eventloop.handle();
            let repeat = calloop::timer::Timer::<CachedKeyPress>::new().unwrap();
            handle
                .insert_source(rx, {
                    let repeater = repeat.handle();
                    move |event, _ignored, state| {
                        let event = match event {
                            calloop::channel::Event::Closed => {
                                tracing::info!("keyboard event loop closed shutting down");
                                state.0.stop();
                                return;
                            }
                            calloop::channel::Event::Msg(keyevent) => keyevent,
                        };
                        state.1.consume(event.0, event.1, event.2);
                        match &state.1.last_key_press {
                            None => repeater.cancel_all_timeouts(),
                            Some(cached) => {
                                repeater.cancel_all_timeouts();
                                repeater.add_timeout(state.1.repeat.delay, cached.clone());
                            }
                        };
                    }
                })
                .unwrap();

            // generate repeat keypresses.
            handle
                .insert_source(repeat, |event, timer, state| {
                    timer.add_timeout(state.1.repeat.rate, event.clone());
                    state.1.keystroke(&event);
                })
                .unwrap();

            tracing::debug!("keyboard event loop initiated");
            eventloop
                .run(
                    std::time::Duration::from_secs(60),
                    &mut (signal, Keyboard::default()),
                    |_ignored| {
                        tracing::trace!("keyboard event loop idle");
                    },
                )
                .expect("keyboard event processing failed");
            tracing::debug!("keyboard event loop completed");
        });

        state
    }
}

struct ModMap(u32, Modifiers);

impl ModMap {
    fn merge(self, m: Modifiers, mods: u32, locked: u32) -> Modifiers {
        if self.0 & mods == 0 && self.0 & locked == 0 {
            return m;
        }

        m | self.1
    }
}

const MOD_SHIFT: ModMap = ModMap(1, Modifiers::SHIFT);
const MOD_CAP_LOCK: ModMap = ModMap(2, Modifiers::CAPS_LOCK);
const MOD_CTRL: ModMap = ModMap(4, Modifiers::CONTROL);
const MOD_ALT: ModMap = ModMap(8, Modifiers::ALT);
const MOD_NUM_LOCK: ModMap = ModMap(16, Modifiers::NUM_LOCK);
const MOD_META: ModMap = ModMap(64, Modifiers::META);

pub fn event_to_mods(event: wl_keyboard::Event) -> Modifiers {
    match event {
        wl_keyboard::Event::Modifiers {
            mods_depressed,
            mods_locked,
            ..
        } => {
            let mods = Modifiers::empty();
            let mods = MOD_SHIFT.merge(mods, mods_depressed, mods_locked);
            let mods = MOD_CAP_LOCK.merge(mods, mods_depressed, mods_locked);
            let mods = MOD_CTRL.merge(mods, mods_depressed, mods_locked);
            let mods = MOD_ALT.merge(mods, mods_depressed, mods_locked);
            let mods = MOD_NUM_LOCK.merge(mods, mods_depressed, mods_locked);

            MOD_META.merge(mods, mods_depressed, mods_locked)
        }
        _ => Modifiers::empty(),
    }
}

pub struct Manager {
    inner: std::sync::Arc<State>,
}

impl Default for Manager {
    fn default() -> Self {
        Self {
            inner: std::sync::Arc::new(State::default()),
        }
    }
}

// impl Manager {
//     pub(super) fn attach(
//         &self,
//         id: u32,
//         seat: wlc::Main<wl_seat::WlSeat>,
//     ) -> wlc::Main<wl_keyboard::WlKeyboard> {
//         let keyboard = seat.get_keyboard();
//         keyboard.quick_assign({
//             let tx = self.inner.tx.clone();
//             let queue = self.inner.apptx.clone();
//             move |_, event, _| {
//                 if let Err(cause) = tx.send((id, event, queue.clone())) {
//                     tracing::error!("failed to transmit keyboard event {:?}", cause);
//                 };
//             }
//         });

//         keyboard
//     }

//     // TODO turn struct into a calloop event source.
//     pub(super) fn events(&self, handle: &calloop::LoopHandle<std::sync::Arc<Data>>) {
//         let rx = self.inner.apprx.borrow_mut().take().unwrap();
//         handle
//             .insert_source(rx, {
//                 move |evt, _ignored, appdata| {
//                     let evt = match evt {
//                         calloop::channel::Event::Msg(e) => e,
//                         calloop::channel::Event::Closed => {
//                             tracing::info!("keyboard events receiver closed");
//                             return;
//                         }
//                     };

//                     if let Some(winhandle) = appdata.acquire_current_window() {
//                         if let Some(windata) = winhandle.data() {
//                             windata.with_handler({
//                                 let windata = windata.clone();
//                                 let evt = evt;
//                                 move |handler| match evt.state {
//                                     KeyState::Up => {
//                                         handler.key_up(evt.clone());
//                                         tracing::trace!(
//                                             "key press event up {:?} {:?}",
//                                             evt,
//                                             windata.active_text_input.get()
//                                         );
//                                     }
//                                     KeyState::Down => {
//                                         let handled = text::simulate_input(
//                                             handler,
//                                             windata.active_text_input.get(),
//                                             evt.clone(),
//                                         );
//                                         tracing::trace!(
//                                             "key press event down {:?} {:?} {:?}",
//                                             handled,
//                                             evt,
//                                             windata.active_text_input.get()
//                                         );
//                                     }
//                                 }
//                             });
//                         }
//                     }
//                 }
//             })
//             .unwrap();
//     }
// }
