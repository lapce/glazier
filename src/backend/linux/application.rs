#[cfg(feature = "wayland")]
use crate::backend::wayland;
#[cfg(feature = "x11")]
use crate::backend::x11;
use crate::AppHandler;

use super::clipboard::Clipboard;

#[derive(Clone)]
pub(crate) enum Application {
    #[cfg(feature = "x11")]
    X11(x11::application::Application),
    #[cfg(feature = "wayland")]
    Wayland(wayland::application::Application),
}

impl Application {
    pub fn new() -> Result<Self, anyhow::Error> {
        #[cfg(feature = "wayland")]
        if let Ok(app) = wayland::application::Application::new() {
            return Ok(Application::Wayland(app));
        }

        #[cfg(feature = "x11")]
        if let Ok(app) = x11::application::Application::new() {
            return Ok(Application::X11(app));
        }

        Err(anyhow::anyhow!("can't create application"))
    }

    pub fn quit(&self) {
        match self {
            Application::X11(app) => {
                app.quit();
            }
            Application::Wayland(app) => {
                app.quit();
            }
        }
    }

    pub fn clipboard(&self) -> Clipboard {
        match self {
            Application::X11(app) => Clipboard::X11(app.clipboard()),
            Application::Wayland(app) => Clipboard::Wayland(app.clipboard()),
        }
    }

    pub fn get_locale() -> String {
        let app = crate::Application::try_global().unwrap();
        match &app.backend_app {
            Application::X11(_app) => x11::application::Application::get_locale(),
            Application::Wayland(_app) => wayland::application::Application::get_locale(),
        }
    }

    pub fn run(self, handler: Option<Box<dyn AppHandler>>) {
        match self {
            Application::X11(app) => {
                app.run(handler);
            }
            Application::Wayland(app) => {
                app.run(handler);
            }
        }
    }
}
