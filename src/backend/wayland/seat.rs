use std::sync::Arc;

use smithay_client_toolkit::{
    reexports::protocols::wp::relative_pointer::zv1::client::zwp_relative_pointer_v1::ZwpRelativePointerV1,
    seat::pointer::ThemedPointer,
};

use crate::Modifiers;

use super::pointer::GlazierPointerData;

#[derive(Debug)]
pub(super) struct GlazierSeatState {
    /// The pointer bound on the seat.
    pub(super) pointer: Option<Arc<ThemedPointer<GlazierPointerData>>>,

    /// The relative pointer bound on the seat.
    pub(super) relative_pointer: Option<ZwpRelativePointerV1>,

    /// The current modifiers state on the seat.
    pub(super) modifiers: Modifiers,
}

impl GlazierSeatState {
    pub(super) fn new() -> Self {
        Self {
            pointer: None,
            relative_pointer: None,
            modifiers: Modifiers::empty(),
        }
    }
}
