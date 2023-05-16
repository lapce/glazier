#[cfg(feature = "wayland")]
use crate::backend::wayland;
#[cfg(feature = "x11")]
use crate::backend::x11;
use crate::{ClipboardFormat, FormatId};

#[derive(Debug, Clone)]
pub enum Clipboard {
    #[cfg(feature = "x11")]
    X11(x11::clipboard::Clipboard),
    #[cfg(feature = "wayland")]
    Wayland(wayland::clipboard::Clipboard),
}

impl Clipboard {
    pub fn put_string(&mut self, s: impl AsRef<str>) {
        match self {
            Clipboard::X11(clipboard) => {
                clipboard.put_string(s);
            }
            Clipboard::Wayland(clipboard) => {
                clipboard.put_string(s);
            }
        }
    }

    pub fn put_formats(&mut self, formats: &[ClipboardFormat]) {
        match self {
            Clipboard::X11(clipboard) => {
                clipboard.put_formats(formats);
            }
            Clipboard::Wayland(clipboard) => {
                clipboard.put_formats(formats);
            }
        }
    }

    pub fn get_string(&self) -> Option<String> {
        match self {
            Clipboard::X11(clipboard) => clipboard.get_string(),
            Clipboard::Wayland(clipboard) => clipboard.get_string(),
        }
    }

    pub fn preferred_format(&self, formats: &[FormatId]) -> Option<FormatId> {
        match self {
            Clipboard::X11(clipboard) => clipboard.preferred_format(formats),
            Clipboard::Wayland(clipboard) => clipboard.preferred_format(formats),
        }
    }

    pub fn get_format(&self, format: FormatId) -> Option<Vec<u8>> {
        match self {
            Clipboard::X11(clipboard) => clipboard.get_format(format),
            Clipboard::Wayland(clipboard) => clipboard.get_format(format),
        }
    }

    pub fn available_type_names(&self) -> Vec<String> {
        match self {
            Clipboard::X11(clipboard) => clipboard.available_type_names(),
            Clipboard::Wayland(clipboard) => clipboard.available_type_names(),
        }
    }
}
