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

//! wayland platform errors.

use smithay_client_toolkit::reexports::client::{
    self,
    globals::{BindError, GlobalError},
};
use std::{error::Error as StdError, fmt, sync::Arc};

#[derive(Debug, Clone)]
pub enum Error {
    /// Error connecting to wayland server.
    Connect(Arc<client::ConnectError>),
    /// A wayland global either doesn't exist, or doesn't support the version we need.
    Global {
        name: String,
        version: u32,
        inner: Arc<GlobalError>,
    },
    Bind {
        name: String,
        inner: Arc<BindError>,
    },
    /// An unexpected error occurred. It's not handled by glazier/wayland, so you should
    /// terminate the app.
    Fatal(Arc<dyn StdError + 'static>),
    String(ErrorString),
    InvalidParent(u32),
    InvalidId,
    /// general error.
    Err(Arc<dyn StdError + 'static>),
}

impl Error {
    #[allow(clippy::self_named_constructors)]
    pub fn error(e: impl StdError + 'static) -> Self {
        Self::Err(Arc::new(e))
    }

    pub fn fatal(e: impl StdError + 'static) -> Self {
        Self::Fatal(Arc::new(e))
    }

    pub fn global(name: impl Into<String>, version: u32, inner: GlobalError) -> Self {
        Error::Global {
            name: name.into(),
            version,
            inner: Arc::new(inner),
        }
    }

    pub fn bind(name: impl Into<String>, inner: BindError) -> Self {
        Error::Bind {
            name: name.into(),
            inner: Arc::new(inner),
        }
    }

    pub fn string(s: impl Into<String>) -> Self {
        Error::String(ErrorString::from(s))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        match self {
            Self::Connect(e) => write!(f, "could not connect to the wayland server: {e:?}"),
            Self::Global { name, version, .. } => write!(
                f,
                "a required wayland global ({name}@{version}) was unavailable"
            ),
            Self::Fatal(e) => write!(f, "an unhandled error occurred: {e:?}"),
            Self::Err(e) => write!(f, "an unhandled error occurred: {e:?}"),
            Self::String(e) => e.fmt(f),
            Self::InvalidParent(id) => write!(f, "invalid parent window for popup: {id:?}"),
            Self::InvalidId => write!(f, "Invalid ObjectId"),
            Self::Bind { name, inner } => write!(f, "{name} failed to bind: {inner}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Connect(e) => Some(&**e),
            Self::Global { inner, .. } => Some(&**inner),
            Self::Fatal(e) => Some(&**e),
            Self::Err(e) => Some(&**e),
            Self::String(e) => Some(e),
            Self::InvalidParent(_) => None,
            Self::InvalidId => None,
            Self::Bind { inner, .. } => Some(inner),
        }
    }
}

impl From<client::ConnectError> for Error {
    fn from(err: client::ConnectError) -> Self {
        Self::Connect(Arc::new(err))
    }
}

impl From<smithay_client_toolkit::reexports::client::backend::InvalidId> for Error {
    fn from(_: smithay_client_toolkit::reexports::client::backend::InvalidId) -> Self {
        Error::InvalidId
    }
}

#[derive(Debug, Clone)]
pub struct ErrorString {
    details: String,
}

impl ErrorString {
    pub fn from(s: impl Into<String>) -> Self {
        Self { details: s.into() }
    }
}

impl std::fmt::Display for ErrorString {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.details)
    }
}

impl std::error::Error for ErrorString {
    fn description(&self) -> &str {
        &self.details
    }
}
