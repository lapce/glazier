//! Relative pointer.

use std::ops::Deref;

use smithay_client_toolkit::globals::GlobalData;
use smithay_client_toolkit::reexports::client::globals::{BindError, GlobalList};
use smithay_client_toolkit::reexports::client::{delegate_dispatch, Dispatch};
use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};
use smithay_client_toolkit::reexports::protocols::wp::relative_pointer::zv1::{
    client::zwp_relative_pointer_manager_v1::ZwpRelativePointerManagerV1,
    client::zwp_relative_pointer_v1::ZwpRelativePointerV1,
};

use super::application::Data;

/// Wrapper around the relative pointer.
pub struct RelativePointerState {
    manager: ZwpRelativePointerManagerV1,
}

impl RelativePointerState {
    /// Create new relative pointer manager.
    pub fn new(globals: &GlobalList, queue_handle: &QueueHandle<Data>) -> Result<Self, BindError> {
        let manager = globals.bind(queue_handle, 1..=1, GlobalData)?;
        Ok(Self { manager })
    }
}

impl Deref for RelativePointerState {
    type Target = ZwpRelativePointerManagerV1;

    fn deref(&self) -> &Self::Target {
        &self.manager
    }
}

impl Dispatch<ZwpRelativePointerManagerV1, GlobalData, Data> for RelativePointerState {
    fn event(
        _state: &mut Data,
        _proxy: &ZwpRelativePointerManagerV1,
        _event: <ZwpRelativePointerManagerV1 as wayland_client::Proxy>::Event,
        _data: &GlobalData,
        _conn: &Connection,
        _qhandle: &QueueHandle<Data>,
    ) {
    }
}

impl Dispatch<ZwpRelativePointerV1, GlobalData, Data> for RelativePointerState {
    fn event(
        state: &mut Data,
        _proxy: &ZwpRelativePointerV1,
        event: <ZwpRelativePointerV1 as wayland_client::Proxy>::Event,
        _data: &GlobalData,
        _conn: &Connection,
        _qhandle: &QueueHandle<Data>,
    ) {
        // if let zwp_relative_pointer_v1::Event::RelativeMotion {
        //     dx_unaccel,
        //     dy_unaccel,
        //     ..
        // } = event
        // {
        //     state.events_sink.push_device_event(
        //         DeviceEvent::MouseMotion {
        //             delta: (dx_unaccel, dy_unaccel),
        //         },
        //         super::DeviceId,
        //     );
        // }
    }
}

delegate_dispatch!(Data: [ZwpRelativePointerV1: GlobalData] => RelativePointerState);
delegate_dispatch!(Data: [ZwpRelativePointerManagerV1: GlobalData] => RelativePointerState);
