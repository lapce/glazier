use std::{
    cell::RefCell,
    sync::{Arc, Mutex},
};

use kurbo::{Point, Vec2};
use smithay_client_toolkit::{
    compositor::SurfaceData,
    reexports::client::{
        delegate_dispatch,
        protocol::{wl_pointer::WlPointer, wl_seat::WlSeat, wl_surface::WlSurface},
    },
    seat::{
        pointer::{PointerData, PointerDataExt, PointerEventKind, PointerHandler},
        SeatState,
    },
    shell::xdg::frame::FrameClick,
};
use wayland_client::Proxy;

use crate::{pointer::MouseInfo, MouseButton, PointerEvent, PointerType};

use super::{application::Data, window::make_wid};

#[derive(Debug)]
pub(super) struct GlazierPointerDataInner {
    /// Currently focused window.
    surface: Option<u64>,

    /// Serial of the last button event.
    latest_button_serial: u32,
}

impl Default for GlazierPointerDataInner {
    fn default() -> Self {
        Self {
            surface: None,
            latest_button_serial: 0,
        }
    }
}

#[derive(Debug)]
pub(super) struct GlazierPointerData {
    /// The surface associated with this pointer, which is used for icons.
    cursor_surface: WlSurface,

    /// The inner winit data associated with the pointer.
    inner: Mutex<GlazierPointerDataInner>,

    /// The data required by the sctk.
    sctk_data: PointerData,
}

impl PointerHandler for Data {
    fn pointer_frame(
        &mut self,
        conn: &smithay_client_toolkit::reexports::client::Connection,
        qh: &smithay_client_toolkit::reexports::client::QueueHandle<Self>,
        pointer: &smithay_client_toolkit::reexports::client::protocol::wl_pointer::WlPointer,
        events: &[smithay_client_toolkit::seat::pointer::PointerEvent],
    ) {
        let seat = pointer.glazier_data().seat();
        let seat_state = self.seats.get(&seat.id()).unwrap();
        let modifiers = seat_state.modifiers;

        for event in events {
            let surface = &event.surface;

            // The parent surface.
            let parent_surface = match event.surface.data::<SurfaceData>() {
                Some(data) => data.parent_surface().unwrap_or(surface),
                None => continue,
            };

            let window_id = make_wid(parent_surface);
            let mut handles = self.handles.borrow_mut();
            let handle = match handles.get_mut(&window_id) {
                Some(handle) => handle,
                None => continue,
            };

            let position = Point::new(event.position.0, event.position.1);

            match event.kind {
                // Pointer movements on decorations.
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. }
                    if parent_surface != surface =>
                {
                    if let Some(icon) =
                        handle.frame_point_moved(surface, event.position.0, event.position.1)
                    {
                        if let Some(pointer) = seat_state.pointer.as_ref() {
                            let surface = pointer
                                .pointer()
                                .data::<GlazierPointerData>()
                                .unwrap()
                                .cursor_surface();
                            let scale_factor =
                                surface.data::<SurfaceData>().unwrap().scale_factor();

                            let _ = pointer.set_cursor(
                                conn,
                                &icon,
                                self.shm.wl_shm(),
                                surface,
                                scale_factor,
                            );
                        }
                    }
                }
                PointerEventKind::Leave { .. } if parent_surface != surface => {
                    handle.frame_point_left();
                }
                ref kind @ PointerEventKind::Press { button, serial, .. }
                | ref kind @ PointerEventKind::Release { button, serial, .. }
                    if parent_surface != surface =>
                {
                    let click = match wayland_button_to_glazier(button) {
                        MouseButton::Left => FrameClick::Normal,
                        MouseButton::Right => FrameClick::Alternate,
                        _ => continue,
                    };
                    let pressed = matches!(kind, PointerEventKind::Press { .. });

                    // Emulate click on the frame.
                    handle.frame_click(
                        click,
                        pressed,
                        seat,
                        serial,
                        window_id,
                        &mut self.window_compositor_updates,
                    );
                }
                // Regular events on the main surface.
                PointerEventKind::Enter { .. } => {
                    // Set the currently focused surface.
                    pointer.glazier_data().inner.lock().unwrap().surface = Some(window_id);

                    if let Some(pointer) = seat_state.pointer.as_ref().map(Arc::downgrade) {
                        handle.pointer_entered(pointer, position, modifiers);
                    }
                }
                PointerEventKind::Leave { .. } => {
                    // Remove the active surface.
                    pointer.glazier_data().inner.lock().unwrap().surface = None;

                    if let Some(pointer) = seat_state.pointer.as_ref().map(Arc::downgrade) {
                        handle.pointer_left(pointer);
                    }
                }
                PointerEventKind::Motion { .. } => {
                    if let Some(handler) = handle.handler() {
                        handler.borrow_mut().pointer_move(&PointerEvent {
                            pos: position,
                            modifiers,
                            ..Default::default()
                        });
                    }
                }
                ref kind @ PointerEventKind::Press { button, serial, .. }
                | ref kind @ PointerEventKind::Release { button, serial, .. } => {
                    // Update the last button serial.
                    pointer
                        .glazier_data()
                        .inner
                        .lock()
                        .unwrap()
                        .latest_button_serial = serial;

                    let button = wayland_button_to_glazier(button);
                    if let Some(handler) = handle.handler() {
                        let event = PointerEvent {
                            button: button.into(),
                            pos: position,
                            modifiers,
                            ..Default::default()
                        };
                        if matches!(kind, PointerEventKind::Press { .. }) {
                            handler.borrow_mut().pointer_down(&event);
                        } else {
                            handler.borrow_mut().pointer_up(&event);
                        }
                    }
                }
                PointerEventKind::Axis {
                    horizontal,
                    vertical,
                    ..
                } => {
                    let has_discrete_scroll = horizontal.discrete != 0 || vertical.discrete != 0;

                    let delta = if has_discrete_scroll {
                        Vec2::new(-horizontal.discrete as f64, -vertical.discrete as f64)
                    } else {
                        Vec2::new(-horizontal.absolute, -vertical.absolute)
                    };

                    let event = PointerEvent {
                        pos: position,
                        modifiers,
                        pointer_type: PointerType::Mouse(MouseInfo { wheel_delta: delta }),
                        ..Default::default()
                    };

                    if let Some(handler) = handle.handler() {
                        handler.borrow_mut().wheel(&event);
                    }
                }
            }
        }
    }
}

impl PointerDataExt for GlazierPointerData {
    fn pointer_data(&self) -> &PointerData {
        &self.sctk_data
    }
}

pub(super) trait GlazierPointerDataExt {
    fn glazier_data(&self) -> &GlazierPointerData;
}

impl GlazierPointerDataExt for WlPointer {
    fn glazier_data(&self) -> &GlazierPointerData {
        self.data::<GlazierPointerData>()
            .expect("failed to get pointer data.")
    }
}

impl GlazierPointerData {
    pub fn new(seat: WlSeat, surface: WlSurface) -> Self {
        Self {
            cursor_surface: surface,
            inner: Mutex::new(GlazierPointerDataInner::default()),
            sctk_data: PointerData::new(seat),
        }
    }

    /// Seat associated with this pointer.
    pub fn seat(&self) -> &WlSeat {
        self.sctk_data.seat()
    }

    /// The WlSurface used to set cursor theme.
    pub fn cursor_surface(&self) -> &WlSurface {
        &self.cursor_surface
    }

    /// Last enter serial.
    pub fn latest_enter_serial(&self) -> u32 {
        self.sctk_data.latest_enter_serial().unwrap_or_default()
    }
}

/// Convert the Wayland button into glazier.
fn wayland_button_to_glazier(button: u32) -> MouseButton {
    // These values are comming from <linux/input-event-codes.h>.
    const BTN_LEFT: u32 = 0x110;
    const BTN_RIGHT: u32 = 0x111;
    const BTN_MIDDLE: u32 = 0x112;

    match button {
        BTN_LEFT => MouseButton::Left,
        BTN_RIGHT => MouseButton::Right,
        BTN_MIDDLE => MouseButton::Middle,
        button => MouseButton::None,
    }
}

delegate_dispatch!(Data: [ WlPointer: GlazierPointerData] => SeatState);
