//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#![allow(clippy::single_match)]

use std::cell::RefCell;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::Arc;

use sctk_adwaita::AdwaitaFrame;
use smithay_client_toolkit::{
    compositor::{CompositorState, Region},
    reexports::{
        client::{protocol::wl_surface::WlSurface, Proxy, QueueHandle},
        protocols::wp::viewporter::client::wp_viewport::WpViewport,
    },
    shell::{
        xdg::{
            frame::DecorationsFrame,
            window::{DecorationMode, Window, WindowConfigure, WindowDecorations},
            XdgSurface,
        },
        WaylandSurface,
    },
    shm::Shm,
    subcompositor::SubcompositorState,
};
use tracing;
use wayland_protocols::xdg_shell::client::xdg_popup;
use wayland_protocols::xdg_shell::client::xdg_positioner;
use wayland_protocols::xdg_shell::client::xdg_surface;

use raw_window_handle::{
    HasRawDisplayHandle, HasRawWindowHandle, RawDisplayHandle, RawWindowHandle,
    WaylandDisplayHandle, WaylandWindowHandle,
};

use super::application::Data;
use super::application::{self, Timer};
use super::surfaces::idle;
use super::{error::Error, menu::Menu, outputs, surfaces};

use crate::{
    dialog::FileDialogOptions,
    error::Error as ShellError,
    kurbo::{Insets, Point, Rect, Size},
    mouse::{Cursor, CursorDesc},
    scale::Scale,
    text::Event,
    window::{self, FileDialogToken, TimerToken, WinHandler, WindowLevel},
    TextFieldToken,
};

pub use surfaces::idle::Handle as IdleHandle;

// holds references to the various components for a window implementation.
struct Inner {
    pub(super) id: u64,
    /// Reference to the underlying SCTK window.
    pub(super) window: Window,

    pub(super) queue_handle: QueueHandle<Data>,

    pub(super) compositor_state: Arc<CompositorState>,

    /// The window frame, which is created from the configure request.
    frame: Option<AdwaitaFrame<Data>>,

    /// The last received configure.
    pub last_configure: Option<WindowConfigure>,

    /// The current window title.
    title: String,

    /// Whether the window is transparent.
    transparent: bool,

    /// The inner size of the window, as in without client side decorations.
    size: Size,

    /// Whether the CSD fail to create, so we don't try to create them on each iteration.
    csd_fails: bool,

    /// The size of the window when no states were applied to it. The primary use for it
    /// is to fallback to original window size, before it was maximized, if the compositor
    /// sends `None` for the new size in the configure.
    stateless_size: Size,

    viewport: Option<WpViewport>,

    // pub(super) decor: Box<dyn surfaces::Decor>,
    // pub(super) surface: Box<dyn surfaces::Handle>,
    // pub(super) outputs: Box<dyn surfaces::Outputs>,
    // pub(super) popup: Box<dyn surfaces::Popup>,
    pub(super) appdata: Rc<RefCell<application::Data>>,

    idle_queue: std::sync::Arc<std::sync::Mutex<Vec<idle::Kind>>>,
}

impl Inner {
    /// Reissue the transparency hint to the compositor.
    pub fn reload_transparency_hint(&self) {
        let surface = self.window.wl_surface();

        if self.transparent {
            surface.set_opaque_region(None);
        } else if let Ok(region) = Region::new(&*self.compositor_state) {
            region.add(0, 0, i32::MAX, i32::MAX);
            surface.set_opaque_region(Some(region.wl_region()));
        } else {
            tracing::trace!("Failed to mark window opaque.");
        }
    }
}

#[derive(Clone)]
pub struct WindowHandle {
    inner: Rc<RefCell<Option<Inner>>>,
}

impl surfaces::Outputs for WindowHandle {
    fn removed(&self, o: &outputs::Meta) {}

    fn inserted(&self, o: &outputs::Meta) {}
}

impl surfaces::Popup for WindowHandle {
    fn surface(
        &self,
        popup: &wayland_client::Main<xdg_surface::XdgSurface>,
        pos: &wayland_client::Main<xdg_positioner::XdgPositioner>,
    ) -> Result<wayland_client::Main<xdg_popup::XdgPopup>, Error> {
        Err(Error::string("no popup"))
    }
}

impl WindowHandle {
    pub(super) fn new(
        outputs: impl Into<Box<dyn surfaces::Outputs>>,
        decor: impl Into<Box<dyn surfaces::Decor>>,
        surface: impl Into<Box<dyn surfaces::Handle>>,
        popup: impl Into<Box<dyn surfaces::Popup>>,
        appdata: Rc<RefCell<application::Data>>,
    ) -> Self {
        Self {
            inner: Rc::new(RefCell::new(None)),
        }
    }

    pub fn configure(
        &self,
        configure: WindowConfigure,
        shm: &Shm,
        subcompositor: &Arc<SubcompositorState>,
    ) -> Size {
        let new_size = {
            let mut inner = self.inner.borrow_mut();
            let inner = inner.as_mut().unwrap();
            if configure.decoration_mode == DecorationMode::Client
                && inner.frame.is_none()
                && !inner.csd_fails
            {
                match AdwaitaFrame::new(
                    &inner.window,
                    shm,
                    subcompositor.clone(),
                    inner.queue_handle.clone(),
                    sctk_adwaita::FrameConfig::auto(),
                ) {
                    Ok(mut frame) => {
                        frame.set_title(&inner.title);
                        // Ensure that the frame is not hidden.
                        frame.set_hidden(false);
                        inner.frame = Some(frame);
                    }
                    Err(err) => {
                        tracing::trace!("Failed to create client side decorations frame: {err}");
                        inner.csd_fails = true;
                    }
                }
            } else if configure.decoration_mode == DecorationMode::Server {
                // Drop the frame for server side decorations to save resources.
                inner.frame = None;
            }

            let stateless = Self::is_stateless(&configure);

            let new_size = if let Some(frame) = inner.frame.as_mut() {
                // Configure the window states.
                frame.update_state(configure.state);

                match configure.new_size {
                    (Some(width), Some(height)) => {
                        let (width, height) = frame.subtract_borders(width, height);
                        (
                            width.map(|w| w.get()).unwrap_or(1) as f64,
                            height.map(|h| h.get()).unwrap_or(1) as f64,
                        )
                            .into()
                    }
                    (_, _) if stateless => inner.stateless_size,
                    _ => inner.size,
                }
            } else {
                match configure.new_size {
                    (Some(width), Some(height)) => (width.get() as f64, height.get() as f64).into(),
                    _ if stateless => inner.stateless_size,
                    _ => inner.size,
                }
            };

            // XXX Set the configure before doing a resize.
            inner.last_configure = Some(configure);

            new_size
        };

        // XXX Update the new size right away.
        self.set_size(new_size);

        new_size
    }

    pub fn is_configured(&self) -> bool {
        let inner = self.inner.borrow();
        inner
            .as_ref()
            .map(|inner| inner.last_configure.is_some())
            .unwrap_or(false)
    }

    #[inline]
    fn is_stateless(configure: &WindowConfigure) -> bool {
        !(configure.is_maximized() || configure.is_fullscreen() || configure.is_tiled())
    }

    pub fn id(&self) -> u64 {
        self.inner.borrow().as_ref().unwrap().id
    }

    pub fn show(&self) {
        tracing::debug!("show initiated");
    }

    pub fn resizable(&self, _resizable: bool) {
        tracing::warn!("resizable is unimplemented on wayland");
    }

    pub fn show_titlebar(&self, _show_titlebar: bool) {
        tracing::warn!("show_titlebar is unimplemented on wayland");
    }

    pub fn set_position(&self, _position: Point) {
        tracing::warn!("set_position is unimplemented on wayland");
    }

    pub fn get_position(&self) -> Point {
        tracing::warn!("get_position is unimplemented on wayland");
        Point::ZERO
    }

    pub fn content_insets(&self) -> Insets {
        Insets::from(0.)
    }

    pub fn set_size(&self, size: Size) {
        if let Some(inner) = self.inner.borrow_mut().as_mut() {
            inner.size = size;

            // Update the stateless size.
            if Some(true) == inner.last_configure.as_ref().map(Self::is_stateless) {
                inner.stateless_size = size;
            }

            // Update the inner frame.
            let ((x, y), outer_size) = if let Some(frame) = inner.frame.as_mut() {
                // Resize only visible frame.
                if !frame.is_hidden() {
                    frame.resize(
                        NonZeroU32::new(inner.size.width as u32).unwrap(),
                        NonZeroU32::new(inner.size.height as u32).unwrap(),
                    );
                }

                (frame.location(), {
                    let (width, height) =
                        frame.add_borders(inner.size.width as u32, inner.size.height as u32);
                    (width as f64, height as f64).into()
                })
            } else {
                ((0, 0), inner.size)
            };

            // Reload the hint.
            inner.reload_transparency_hint();

            // Set the window geometry.
            inner.window.xdg_surface().set_window_geometry(
                x,
                y,
                outer_size.width as i32,
                outer_size.height as i32,
            );

            // Update the target viewport, this is used if and only if fractional scaling is in use.
            if let Some(viewport) = inner.viewport.as_ref() {
                // Set inner size without the borders.
                viewport.set_destination(inner.size.width as _, inner.size.height as _);
            }
        }
    }

    pub fn get_size(&self) -> Size {
        if let Some(inner) = self.inner.borrow().as_ref() {
            inner.size
        } else {
            Size::ZERO
        }
    }

    pub fn set_window_state(&mut self, _current_state: window::WindowState) {
        tracing::warn!("set_window_state is unimplemented on wayland");
    }

    pub fn get_window_state(&self) -> window::WindowState {
        tracing::warn!("get_window_state is unimplemented on wayland");
        window::WindowState::Maximized
    }

    pub fn handle_titlebar(&self, _val: bool) {
        tracing::warn!("handle_titlebar is unimplemented on wayland");
    }

    /// Close the window.
    pub fn close(&self) {
        if let Some(inner) = self.inner.borrow().as_ref() {
            let appdata = inner.appdata.borrow();
            tracing::trace!(
                "closing window initiated {:?}",
                appdata.active_surface_id.borrow()
            );
            appdata.handles.borrow_mut().remove(&self.id());
            appdata.active_surface_id.borrow_mut().pop_front();
            inner.window.wl_surface().destroy();
            tracing::trace!(
                "closing window completed {:?}",
                appdata.active_surface_id.borrow()
            );
        }
    }

    /// Bring this window to the front of the window stack and give it focus.
    pub fn bring_to_front_and_focus(&self) {
        tracing::warn!("unimplemented bring_to_front_and_focus initiated");
    }

    /// Request a new paint, but without invalidating anything.
    pub fn request_anim_frame(&self) {
        // self.inner.surface.request_anim_frame();
    }

    /// Request invalidation of the entire window contents.
    pub fn invalidate(&self) {
        // self.inner.surface.invalidate();
    }

    /// Request invalidation of one rectangle, which is given in display points relative to the
    /// drawing area.
    pub fn invalidate_rect(&self, rect: Rect) {
        // self.inner.surface.invalidate_rect(rect);
    }

    pub fn add_text_field(&self) -> TextFieldToken {
        TextFieldToken::next()
    }

    pub fn remove_text_field(&self, token: TextFieldToken) {
        // self.inner.surface.remove_text_field(token);
    }

    pub fn set_focused_text_field(&self, active_field: Option<TextFieldToken>) {
        // self.inner.surface.set_focused_text_field(active_field);
    }

    pub fn update_text_field(&self, _token: TextFieldToken, _update: Event) {
        // noop until we get a real text input implementation
    }

    pub fn request_timer(&self, deadline: std::time::Instant) -> TimerToken {
        let inner = self.inner.borrow();
        let inner = match inner.as_ref() {
            Some(i) => i,
            None => {
                tracing::warn!("requested timer on a window that was destroyed");
                return Timer::new(self.id(), deadline).token();
            }
        };
        let appdata = inner.appdata.borrow();

        let now = instant::Instant::now();
        let mut timers = appdata.timers.borrow_mut();
        let sooner = timers
            .peek()
            .map(|timer| deadline < timer.deadline())
            .unwrap_or(true);

        let timer = Timer::new(self.id(), deadline);
        timers.push(timer);

        // It is possible that the deadline has passed since it was set.
        let timeout = if deadline < now {
            std::time::Duration::ZERO
        } else {
            deadline - now
        };

        if sooner {
            appdata.timer_handle.cancel_all_timeouts();
            appdata.timer_handle.add_timeout(timeout, timer.token());
        }

        timer.token()
    }

    pub fn set_cursor(&mut self, cursor: &Cursor) {
        if let Some(inner) = self.inner.borrow().as_ref() {
            inner.appdata.borrow().set_cursor(cursor);
        }
    }

    pub fn make_cursor(&self, _desc: &CursorDesc) -> Option<Cursor> {
        tracing::warn!("unimplemented make_cursor initiated");
        None
    }

    pub fn open_file(&mut self, _options: FileDialogOptions) -> Option<FileDialogToken> {
        tracing::warn!("unimplemented open_file");
        None
    }

    pub fn save_as(&mut self, _options: FileDialogOptions) -> Option<FileDialogToken> {
        tracing::warn!("unimplemented save_as");
        None
    }

    /// Get a handle that can be used to schedule an idle task.
    pub fn get_idle_handle(&self) -> Option<IdleHandle> {
        let inner = self.inner.borrow();
        inner.as_ref().map(|inner| idle::Handle {
            queue: inner.idle_queue.clone(),
        })
    }

    /// Get the `Scale` of the window.
    pub fn get_scale(&self) -> Result<Scale, ShellError> {
        todo!()
    }

    pub fn set_menu(&self, _menu: Menu) {
        tracing::warn!("set_menu not implement for wayland");
    }

    pub fn show_context_menu(&self, _menu: Menu, _pos: Point) {
        tracing::warn!("show_context_menu not implement for wayland");
    }

    pub fn set_title(&self, title: impl Into<String>) {
        if let Some(inner) = self.inner.borrow().as_ref() {
            inner.window.set_title(title);
        }
    }

    pub(super) fn run_idle(&self) {
        todo!()
    }

    pub(super) fn data(&self) -> Option<std::sync::Arc<surfaces::surface::Data>> {
        todo!()
    }

    #[cfg(feature = "accesskit")]
    pub fn update_accesskit_if_active(
        &self,
        _update_factory: impl FnOnce() -> accesskit::TreeUpdate,
    ) {
        // AccessKit doesn't yet support this backend.
    }
}

impl PartialEq for WindowHandle {
    fn eq(&self, rhs: &Self) -> bool {
        self.id() == rhs.id()
    }
}

impl Eq for WindowHandle {}

impl Default for WindowHandle {
    fn default() -> WindowHandle {
        WindowHandle {
            inner: Rc::new(RefCell::new(None)),
        }
    }
}

unsafe impl HasRawWindowHandle for WindowHandle {
    fn raw_window_handle(&self) -> RawWindowHandle {
        tracing::error!("HasRawWindowHandle trait not implemented for wasm.");
        RawWindowHandle::Wayland(WaylandWindowHandle::empty())
    }
}

unsafe impl HasRawDisplayHandle for WindowHandle {
    fn raw_display_handle(&self) -> RawDisplayHandle {
        tracing::error!("HasDisplayHandle trait not implemented for wayland.");
        RawDisplayHandle::Wayland(WaylandDisplayHandle::empty())
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct CustomCursor;

/// Builder abstraction for creating new windows
pub(crate) struct WindowBuilder {
    app: application::Application,
    handler: Option<Box<dyn WinHandler>>,
    title: String,
    menu: Option<Menu>,
    position: Option<Point>,
    level: WindowLevel,
    state: Option<window::WindowState>,
    // pre-scaled
    size: Size,
    min_size: Option<Size>,
    resizable: bool,
    show_titlebar: bool,
}

impl WindowBuilder {
    pub fn new(app: application::Application) -> WindowBuilder {
        WindowBuilder {
            app,
            handler: None,
            title: String::new(),
            menu: None,
            size: Size::new(0.0, 0.0),
            position: None,
            level: WindowLevel::AppWindow,
            state: None,
            min_size: None,
            resizable: true,
            show_titlebar: true,
        }
    }

    pub fn handler(mut self, handler: Box<dyn WinHandler>) -> Self {
        self.handler = Some(handler);
        self
    }

    pub fn size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }

    pub fn min_size(mut self, size: Size) -> Self {
        self.min_size = Some(size);
        self
    }

    pub fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    pub fn show_titlebar(mut self, show_titlebar: bool) -> Self {
        self.show_titlebar = show_titlebar;
        self
    }

    pub fn transparent(self, _transparent: bool) -> Self {
        tracing::warn!(
            "WindowBuilder::transparent is unimplemented for Wayland, it allows transparency by default"
        );
        self
    }

    pub fn position(mut self, position: Point) -> Self {
        self.position = Some(position);
        self
    }

    pub fn level(mut self, level: WindowLevel) -> Self {
        self.level = level;
        self
    }

    pub fn window_state(mut self, state: window::WindowState) -> Self {
        self.state = Some(state);
        self
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    pub fn menu(mut self, menu: Menu) -> Self {
        self.menu = Some(menu);
        self
    }

    pub fn build(self) -> Result<WindowHandle, ShellError> {
        if matches!(self.menu, Some(_)) {
            tracing::warn!("menus unimplemented for wayland");
        }

        let level = self.level.clone();

        if let WindowLevel::Modal(parent) = level {
            return self.create_popup(parent);
        }

        if let WindowLevel::DropDown(parent) = level {
            return self.create_popup(parent);
        }

        let mut handler = self.handler.expect("must set a window handler");

        let mut appdata = self.app.data.borrow_mut();
        let window = {
            let surface = appdata
                .compositor_state
                .create_surface(&appdata.queue_handle);
            appdata.xdg_shell.create_window(
                surface,
                WindowDecorations::ServerDefault,
                &appdata.queue_handle,
            )
        };

        window.set_title(self.title.clone());

        window.commit();

        let handle = WindowHandle {
            inner: Rc::new(RefCell::new(Some(Inner {
                id: make_wid(window.wl_surface()),
                window,
                queue_handle: appdata.queue_handle.clone(),
                compositor_state: appdata.compositor_state.clone(),
                frame: None,
                last_configure: None,
                title: self.title,
                transparent: false,
                size: self.size,
                csd_fails: false,
                stateless_size: self.size,
                viewport: None,
                appdata: self.app.data.clone(),
                idle_queue: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
            }))),
        };

        if appdata
            .handles
            .borrow_mut()
            .insert(handle.id(), handle.clone())
            .is_some()
        {
            return Err(ShellError::Platform(
                crate::backend::linux::error::Error::Wayland(Error::string(
                    "wayland should use a unique id",
                )),
            ));
        }

        appdata
            .active_surface_id
            .borrow_mut()
            .push_front(handle.id());

        let mut wayland_source = self.app.wayland_dispatcher.as_source_mut();
        let event_queue = wayland_source.queue();

        // Do a roundtrip.
        event_queue.roundtrip(&mut appdata).map_err(|_| {
            ShellError::Platform(crate::backend::linux::error::Error::Wayland(Error::string(
                "failed to do initial roundtrip for the window.",
            )))
        })?;

        // XXX Wait for the initial configure to arrive.
        while !handle.is_configured() {
            event_queue.blocking_dispatch(&mut appdata).map_err(|_| {
                ShellError::Platform(crate::backend::linux::error::Error::Wayland(Error::string(
                    "failed to dispatch queue while waiting for initial configure.",
                )))
            })?;
        }

        println!("window configured");

        {
            let handle = handle.clone();
            let handle = crate::backend::window::WindowHandle::Wayland(handle);
            handler.connect(&handle.into());
        }

        Ok(handle)
    }

    fn create_popup(self, parent: window::WindowHandle) -> Result<WindowHandle, ShellError> {
        let dim = self.min_size.unwrap_or(Size::ZERO);
        let dim = Size::new(dim.width.max(1.), dim.height.max(1.));
        let dim = Size::new(
            self.size.width.max(dim.width),
            self.size.height.max(dim.height),
        );

        let config = surfaces::popup::Config::default()
            .with_size(dim)
            .with_offset(Into::into(
                self.position.unwrap_or_else(|| Into::into((0., 0.))),
            ));

        tracing::debug!("popup {:?}", config);

        match &parent.0 {
            crate::backend::window::WindowHandle::X11(_) => Err(ShellError::Other(
                anyhow::anyhow!("wrong window handle").into(),
            )),
            crate::backend::window::WindowHandle::Wayland(parent) => {
                popup::create(parent, &config, self.app.data, self.handler)
            }
        }
    }
}

#[allow(unused)]
pub mod layershell {
    use std::cell::RefCell;
    use std::rc::Rc;

    use crate::error::Error as ShellError;
    use crate::window::WinHandler;

    use super::WindowHandle;
    use crate::backend::wayland::application::{Application, Data};
    use crate::backend::wayland::error::Error;
    use crate::backend::wayland::surfaces;

    /// Builder abstraction for creating new windows
    pub(crate) struct Builder {
        appdata: Rc<RefCell<Data>>,
        winhandle: Option<Box<dyn WinHandler>>,
        pub(crate) config: surfaces::layershell::Config,
    }

    impl Builder {
        pub fn new(app: Application) -> Builder {
            Builder {
                appdata: app.data,
                config: surfaces::layershell::Config::default(),
                winhandle: None,
            }
        }

        pub fn set_handler(&mut self, handler: Box<dyn WinHandler>) {
            self.winhandle = Some(handler);
        }

        pub fn build(self) -> Result<WindowHandle, ShellError> {
            let appdata = self.appdata.clone();

            let winhandle = match self.winhandle {
                Some(winhandle) => winhandle,
                None => {
                    return Err(ShellError::Platform(
                        crate::backend::linux::error::Error::Wayland(Error::string(
                            "window handler required",
                        )),
                    ))
                }
            };

            let surface =
                surfaces::layershell::Surface::new(appdata.clone(), winhandle, self.config.clone());

            let handle = WindowHandle::new(
                surface.clone(),
                surfaces::surface::Dead::default(),
                surface.clone(),
                surface.clone(),
                self.appdata.clone(),
            );

            if appdata
                .borrow()
                .handles
                .borrow_mut()
                .insert(handle.id(), handle.clone())
                .is_some()
            {
                panic!("wayland should use unique object IDs");
            }
            appdata
                .borrow()
                .active_surface_id
                .borrow_mut()
                .push_front(handle.id());

            surface.with_handler({
                let handle = handle.clone();
                let handle = crate::backend::window::WindowHandle::Wayland(handle);
                move |winhandle| winhandle.connect(&handle.into())
            });

            Ok(handle)
        }
    }
}

#[allow(unused)]
pub mod popup {
    use std::cell::RefCell;
    use std::rc::Rc;

    use crate::error::Error as ShellError;
    use crate::window::WinHandler;

    use super::WindowBuilder;
    use super::WindowHandle;
    use crate::backend::wayland::application::{Application, Data};
    use crate::backend::wayland::error::Error;
    use crate::backend::wayland::surfaces;

    pub(super) fn create(
        parent: &WindowHandle,
        config: &surfaces::popup::Config,
        wappdata: Rc<RefCell<Data>>,
        winhandle: Option<Box<dyn WinHandler>>,
    ) -> Result<WindowHandle, ShellError> {
        let appdata = wappdata.clone();

        let winhandle = match winhandle {
            Some(winhandle) => winhandle,
            None => {
                return Err(ShellError::Platform(
                    crate::backend::linux::error::Error::Wayland(Error::string(
                        "window handler required",
                    )),
                ))
            }
        };

        // compute the initial window size.
        let updated = config.clone();
        let surface =
            match surfaces::popup::Surface::new(appdata.clone(), winhandle, updated, parent) {
                Err(cause) => {
                    return Err(ShellError::Platform(
                        crate::backend::linux::error::Error::Wayland(cause),
                    ))
                }
                Ok(s) => s,
            };

        let handle = WindowHandle::new(
            surface.clone(),
            surfaces::surface::Dead::default(),
            surface.clone(),
            surface.clone(),
            wappdata,
        );

        if appdata
            .borrow()
            .handles
            .borrow_mut()
            .insert(handle.id(), handle.clone())
            .is_some()
        {
            panic!("wayland should use unique object IDs");
        }
        appdata
            .borrow()
            .active_surface_id
            .borrow_mut()
            .push_front(handle.id());

        surface.with_handler({
            let handle = handle.clone();
            let handle = crate::backend::window::WindowHandle::Wayland(handle);
            move |winhandle| winhandle.connect(&handle.into())
        });

        Ok(handle)
    }
}

// The window update comming from the compositor.
#[derive(Debug, Clone, Copy)]
pub struct WindowCompositorUpdate {
    /// The id of the window this updates belongs to.
    pub window_id: u64,

    /// New window size.
    pub size: Option<Size>,

    /// New scale factor.
    pub scale_factor: Option<f64>,

    /// Close the window.
    pub close_window: bool,
}

impl WindowCompositorUpdate {
    pub fn new(window_id: u64) -> Self {
        Self {
            window_id,
            size: None,
            scale_factor: None,
            close_window: false,
        }
    }
}

/// Get the WindowId out of the surface.
#[inline]
pub(crate) fn make_wid(surface: &WlSurface) -> u64 {
    surface.id().as_ptr() as u64
}
