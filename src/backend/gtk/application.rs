// Copyright 2019 The Druid Authors.
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

//! GTK implementation of features at the application scope.

use gtk4::gdk::Display;
use gtk4::gio::Cancellable;
use gtk4::prelude::{ApplicationExt, ApplicationExtManual, DisplayExt};
use gtk4::traits::GtkApplicationExt;
use gtk4::Application as GtkApplication;

use crate::application::AppHandler;

use super::clipboard::Clipboard;
use super::error::Error;

#[derive(Clone)]
pub(crate) struct Application {
    gtk_app: GtkApplication,
}

impl Application {
    pub fn new() -> Result<Application, Error> {
        // TODO: we should give control over the application ID to the user
        let gtk_app = GtkApplication::builder()
            .application_id("com.github.linebender.druid")
            .build();

        gtk_app.connect_activate(|_app| {
            tracing::info!("gtk: Activated application");
        });

        if let Err(err) = gtk_app.register(None as Option<&Cancellable>) {
            return Err(Error::Error(err));
        }

        Ok(Application { gtk_app })
    }

    #[inline]
    pub fn gtk_app(&self) -> &GtkApplication {
        &self.gtk_app
    }

    pub fn run(self, _handler: Option<Box<dyn AppHandler>>) {
        self.gtk_app.run();
    }

    pub fn quit(&self) {
        match self.gtk_app.active_window() {
            None => {
                // no application is running, main is not running
            }
            Some(_) => {
                // we still have an active window, close the run loop
                self.gtk_app.quit();
            }
        }
    }

    pub fn clipboard(&self) -> Clipboard {
        let display = Display::default().unwrap();
        let clipboard = display.clipboard();
        crate::Clipboard(clipboard)
    }

    pub fn get_locale() -> String {
        let mut locale: String = gtk4::glib::language_names()[0].as_str().into();
        // This is done because the locale parsing library we use expects an unicode locale, but these vars have an ISO locale
        if let Some(idx) = locale.chars().position(|c| c == '.' || c == '@') {
            locale.truncate(idx);
        }
        locale
    }
}

impl crate::platform::linux::ApplicationExt for crate::Application {
    fn primary_clipboard(&self) -> crate::Clipboard {
        crate::Clipboard(self.gtk_app.clipboard())
    }
}
