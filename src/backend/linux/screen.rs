#[cfg(feature = "wayland")]
use crate::backend::wayland;
#[cfg(feature = "x11")]
use crate::backend::x11;
use crate::Monitor;

pub fn get_monitors() -> Vec<Monitor> {
    let app = crate::Application::try_global().unwrap();
    match &app.backend_app {
        super::application::Application::X11(_) => x11::screen::get_monitors(),
        super::application::Application::Wayland(_) => wayland::screen::get_monitors(),
    }
}
