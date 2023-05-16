use kurbo::{Insets, Point, Rect, Size};
use raw_window_handle::{
    HasRawDisplayHandle, HasRawWindowHandle, RawDisplayHandle, RawWindowHandle,
};
use std::time::Instant;

#[cfg(feature = "wayland")]
use crate::backend::wayland;
#[cfg(feature = "x11")]
use crate::backend::x11;
use crate::{
    text::Event, Cursor, CursorDesc, Error, FileDialogOptions, FileDialogToken, IdleToken, Scale,
    TextFieldToken, TimerToken, WinHandler, WindowLevel, WindowState,
};

use super::{application::Application, menu::Menu};

#[derive(Clone, PartialEq, Eq)]
pub enum CustomCursor {
    #[cfg(feature = "x11")]
    X11(x11::window::CustomCursor),
    #[cfg(feature = "wayland")]
    Wayland(wayland::window::CustomCursor),
}

pub(crate) enum WindowBuilder {
    #[cfg(feature = "x11")]
    X11(x11::window::WindowBuilder),
    #[cfg(feature = "wayland")]
    Wayland(wayland::window::WindowBuilder),
}

impl WindowBuilder {
    pub fn new(app: Application) -> Self {
        match app {
            Application::X11(app) => WindowBuilder::X11(x11::window::WindowBuilder::new(app)),
            Application::Wayland(app) => {
                WindowBuilder::Wayland(wayland::window::WindowBuilder::new(app))
            }
        }
    }

    pub fn handler(mut self, handler: Box<dyn WinHandler>) -> Self {
        self = match self {
            WindowBuilder::X11(builder) => WindowBuilder::X11(builder.handler(handler)),
            WindowBuilder::Wayland(builder) => WindowBuilder::Wayland(builder.handler(handler)),
        };
        self
    }

    pub fn size(mut self, size: Size) -> Self {
        self = match self {
            WindowBuilder::X11(builder) => WindowBuilder::X11(builder.size(size)),
            WindowBuilder::Wayland(builder) => WindowBuilder::Wayland(builder.size(size)),
        };
        self
    }

    pub fn min_size(mut self, size: Size) -> Self {
        self = match self {
            WindowBuilder::X11(builder) => WindowBuilder::X11(builder.min_size(size)),
            WindowBuilder::Wayland(builder) => WindowBuilder::Wayland(builder.min_size(size)),
        };
        self
    }

    pub fn resizable(mut self, resizable: bool) -> Self {
        self = match self {
            WindowBuilder::X11(builder) => WindowBuilder::X11(builder.resizable(resizable)),
            WindowBuilder::Wayland(builder) => WindowBuilder::Wayland(builder.resizable(resizable)),
        };
        self
    }

    pub fn show_titlebar(mut self, show_titlebar: bool) -> Self {
        self = match self {
            WindowBuilder::X11(builder) => WindowBuilder::X11(builder.show_titlebar(show_titlebar)),
            WindowBuilder::Wayland(builder) => {
                WindowBuilder::Wayland(builder.show_titlebar(show_titlebar))
            }
        };
        self
    }

    pub fn transparent(mut self, transparent: bool) -> Self {
        self = match self {
            WindowBuilder::X11(builder) => WindowBuilder::X11(builder.transparent(transparent)),
            WindowBuilder::Wayland(builder) => {
                WindowBuilder::Wayland(builder.transparent(transparent))
            }
        };
        self
    }

    pub fn position(mut self, position: Point) -> Self {
        self = match self {
            WindowBuilder::X11(builder) => WindowBuilder::X11(builder.position(position)),
            WindowBuilder::Wayland(builder) => WindowBuilder::Wayland(builder.position(position)),
        };
        self
    }

    pub fn level(mut self, level: WindowLevel) -> Self {
        self = match self {
            WindowBuilder::X11(builder) => WindowBuilder::X11(builder.level(level)),
            WindowBuilder::Wayland(builder) => WindowBuilder::Wayland(builder.level(level)),
        };
        self
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self = match self {
            WindowBuilder::X11(builder) => WindowBuilder::X11(builder.title(title)),
            WindowBuilder::Wayland(builder) => WindowBuilder::Wayland(builder.title(title)),
        };
        self
    }

    pub fn menu(mut self, menu: Menu) -> Self {
        self = match self {
            WindowBuilder::X11(builder) => match menu {
                super::menu::Menu::X11(menu) => WindowBuilder::X11(builder.menu(menu)),
                super::menu::Menu::Wayland(_) => WindowBuilder::X11(builder),
            },
            WindowBuilder::Wayland(builder) => match menu {
                super::menu::Menu::X11(_) => WindowBuilder::Wayland(builder),
                super::menu::Menu::Wayland(menu) => WindowBuilder::Wayland(builder.menu(menu)),
            },
        };
        self
    }

    pub fn window_state(mut self, state: WindowState) -> Self {
        self = match self {
            WindowBuilder::X11(builder) => WindowBuilder::X11(builder.window_state(state)),
            WindowBuilder::Wayland(builder) => WindowBuilder::Wayland(builder.window_state(state)),
        };
        self
    }

    pub fn build(self) -> Result<WindowHandle, Error> {
        match self {
            WindowBuilder::X11(builder) => {
                builder.build().map(WindowHandle::X11).map_err(Into::into)
            }
            WindowBuilder::Wayland(builder) => builder
                .build()
                .map(WindowHandle::Wayland)
                .map_err(Into::into),
        }
    }
}

#[derive(Clone)]
pub enum IdleHandle {
    #[cfg(feature = "x11")]
    X11(x11::window::IdleHandle),
    #[cfg(feature = "wayland")]
    Wayland(wayland::window::IdleHandle),
}

impl IdleHandle {
    pub fn add_idle_callback<F>(&self, callback: F)
    where
        F: FnOnce(&mut dyn WinHandler) + Send + 'static,
    {
        match self {
            IdleHandle::X11(idle) => {
                idle.add_idle_callback(callback);
            }
            IdleHandle::Wayland(idle) => {
                idle.add_idle_callback(callback);
            }
        }
    }

    pub fn add_idle_token(&mut self, token: IdleToken) {
        match self {
            IdleHandle::X11(idle) => {
                idle.add_idle_token(token);
            }
            IdleHandle::Wayland(idle) => {
                idle.add_idle_token(token);
            }
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum WindowHandle {
    #[cfg(feature = "x11")]
    X11(x11::window::WindowHandle),
    #[cfg(feature = "wayland")]
    Wayland(wayland::window::WindowHandle),
}

impl Default for WindowHandle {
    fn default() -> Self {
        Self::Wayland(wayland::window::WindowHandle::default())
    }
}

impl WindowHandle {
    pub fn show(&self) {
        match self {
            WindowHandle::X11(handle) => {
                handle.show();
            }
            WindowHandle::Wayland(handle) => {
                handle.show();
            }
        }
    }

    pub fn close(&self) {
        match self {
            WindowHandle::X11(handle) => {
                handle.close();
            }
            WindowHandle::Wayland(handle) => {
                handle.close();
            }
        }
    }

    pub fn resizable(&self, resizable: bool) {
        match self {
            WindowHandle::X11(handle) => {
                handle.resizable(resizable);
            }
            WindowHandle::Wayland(handle) => {
                handle.resizable(resizable);
            }
        }
    }

    pub fn set_window_state(&mut self, state: WindowState) {
        match self {
            WindowHandle::X11(handle) => {
                handle.set_window_state(state);
            }
            WindowHandle::Wayland(handle) => {
                handle.set_window_state(state);
            }
        }
    }

    pub fn get_window_state(&self) -> WindowState {
        match self {
            WindowHandle::X11(handle) => handle.get_window_state(),
            WindowHandle::Wayland(handle) => handle.get_window_state(),
        }
    }

    pub fn handle_titlebar(&self, val: bool) {
        match self {
            WindowHandle::X11(handle) => {
                handle.handle_titlebar(val);
            }
            WindowHandle::Wayland(handle) => {
                handle.handle_titlebar(val);
            }
        }
    }

    pub fn show_titlebar(&self, show_titlebar: bool) {
        match self {
            WindowHandle::X11(handle) => {
                handle.show_titlebar(show_titlebar);
            }
            WindowHandle::Wayland(handle) => {
                handle.show_titlebar(show_titlebar);
            }
        }
    }

    pub fn set_position(&self, position: Point) {
        match self {
            WindowHandle::X11(handle) => {
                handle.set_position(position);
            }
            WindowHandle::Wayland(handle) => {
                handle.set_position(position);
            }
        }
    }

    pub fn get_position(&self) -> Point {
        match self {
            WindowHandle::X11(handle) => handle.get_position(),
            WindowHandle::Wayland(handle) => handle.get_position(),
        }
    }

    pub fn content_insets(&self) -> Insets {
        match self {
            WindowHandle::X11(handle) => handle.content_insets(),
            WindowHandle::Wayland(handle) => handle.content_insets(),
        }
    }

    pub fn set_size(&self, size: Size) {
        match self {
            WindowHandle::X11(handle) => {
                handle.set_size(size);
            }
            WindowHandle::Wayland(handle) => {
                handle.set_size(size);
            }
        }
    }

    pub fn get_size(&self) -> Size {
        match self {
            WindowHandle::X11(handle) => handle.get_size(),
            WindowHandle::Wayland(handle) => handle.get_size(),
        }
    }

    pub fn bring_to_front_and_focus(&self) {
        match self {
            WindowHandle::X11(handle) => {
                handle.bring_to_front_and_focus();
            }
            WindowHandle::Wayland(handle) => {
                handle.bring_to_front_and_focus();
            }
        }
    }

    pub fn request_anim_frame(&self) {
        match self {
            WindowHandle::X11(handle) => {
                handle.request_anim_frame();
            }
            WindowHandle::Wayland(handle) => {
                handle.request_anim_frame();
            }
        }
    }

    pub fn invalidate(&self) {
        match self {
            WindowHandle::X11(handle) => {
                handle.invalidate();
            }
            WindowHandle::Wayland(handle) => {
                handle.invalidate();
            }
        }
    }

    pub fn invalidate_rect(&self, rect: Rect) {
        match self {
            WindowHandle::X11(handle) => {
                handle.invalidate_rect(rect);
            }
            WindowHandle::Wayland(handle) => {
                handle.invalidate_rect(rect);
            }
        }
    }

    pub fn set_title(&self, title: &str) {
        match self {
            WindowHandle::X11(handle) => {
                handle.set_title(title);
            }
            WindowHandle::Wayland(handle) => {
                handle.set_title(title);
            }
        }
    }

    pub fn set_menu(&self, menu: Menu) {
        match self {
            WindowHandle::X11(handle) => {
                match menu {
                    super::menu::Menu::X11(menu) => {
                        handle.set_menu(menu);
                    }
                    super::menu::Menu::Wayland(_) => {}
                };
            }
            WindowHandle::Wayland(handle) => {
                match menu {
                    super::menu::Menu::X11(_) => {}
                    super::menu::Menu::Wayland(menu) => {
                        handle.set_menu(menu);
                    }
                };
            }
        }
    }

    pub fn add_text_field(&self) -> TextFieldToken {
        match self {
            WindowHandle::X11(handle) => handle.add_text_field(),
            WindowHandle::Wayland(handle) => handle.add_text_field(),
        }
    }

    pub fn remove_text_field(&self, token: TextFieldToken) {
        match self {
            WindowHandle::X11(handle) => {
                handle.remove_text_field(token);
            }
            WindowHandle::Wayland(handle) => {
                handle.remove_text_field(token);
            }
        }
    }

    pub fn set_focused_text_field(&self, active_field: Option<TextFieldToken>) {
        match self {
            WindowHandle::X11(handle) => {
                handle.set_focused_text_field(active_field);
            }
            WindowHandle::Wayland(handle) => {
                handle.set_focused_text_field(active_field);
            }
        }
    }

    pub fn update_text_field(&self, token: TextFieldToken, update: Event) {
        match self {
            WindowHandle::X11(handle) => {
                handle.update_text_field(token, update);
            }
            WindowHandle::Wayland(handle) => {
                handle.update_text_field(token, update);
            }
        }
    }

    pub fn request_timer(&self, deadline: Instant) -> TimerToken {
        match self {
            WindowHandle::X11(handle) => handle.request_timer(deadline),
            WindowHandle::Wayland(handle) => handle.request_timer(deadline),
        }
    }

    pub fn set_cursor(&mut self, cursor: &Cursor) {
        match self {
            WindowHandle::X11(handle) => {
                handle.set_cursor(cursor);
            }
            WindowHandle::Wayland(handle) => {
                handle.set_cursor(cursor);
            }
        }
    }

    pub fn make_cursor(&self, desc: &CursorDesc) -> Option<Cursor> {
        match self {
            WindowHandle::X11(handle) => handle.make_cursor(desc),
            WindowHandle::Wayland(handle) => handle.make_cursor(desc),
        }
    }

    pub fn open_file(&mut self, options: FileDialogOptions) -> Option<FileDialogToken> {
        match self {
            WindowHandle::X11(handle) => handle.open_file(options),
            WindowHandle::Wayland(handle) => handle.open_file(options),
        }
    }

    pub fn save_as(&mut self, options: FileDialogOptions) -> Option<FileDialogToken> {
        match self {
            WindowHandle::X11(handle) => handle.save_as(options),
            WindowHandle::Wayland(handle) => handle.save_as(options),
        }
    }

    pub fn show_context_menu(&self, menu: Menu, pos: Point) {
        match self {
            WindowHandle::X11(handle) => {
                match menu {
                    super::menu::Menu::X11(menu) => {
                        handle.show_context_menu(menu, pos);
                    }
                    super::menu::Menu::Wayland(_) => {}
                };
            }
            WindowHandle::Wayland(handle) => {
                match menu {
                    super::menu::Menu::X11(_) => {}
                    super::menu::Menu::Wayland(menu) => {
                        handle.show_context_menu(menu, pos);
                    }
                };
            }
        }
    }

    pub fn get_idle_handle(&self) -> Option<IdleHandle> {
        match self {
            WindowHandle::X11(handle) => handle.get_idle_handle().map(IdleHandle::X11),
            WindowHandle::Wayland(handle) => handle.get_idle_handle().map(IdleHandle::Wayland),
        }
    }

    pub fn get_scale(&self) -> Result<Scale, Error> {
        match self {
            WindowHandle::X11(handle) => handle.get_scale().map_err(Into::into),
            WindowHandle::Wayland(handle) => handle.get_scale().map_err(Into::into),
        }
    }
}

unsafe impl HasRawWindowHandle for WindowHandle {
    fn raw_window_handle(&self) -> RawWindowHandle {
        match self {
            WindowHandle::X11(handle) => handle.raw_window_handle(),
            WindowHandle::Wayland(handle) => handle.raw_window_handle(),
        }
    }
}

unsafe impl HasRawDisplayHandle for WindowHandle {
    fn raw_display_handle(&self) -> RawDisplayHandle {
        match self {
            WindowHandle::X11(handle) => handle.raw_display_handle(),
            WindowHandle::Wayland(handle) => handle.raw_display_handle(),
        }
    }
}
