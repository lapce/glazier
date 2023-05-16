use std::fmt;

#[derive(Debug, Clone)]
pub enum Error {
    Wayland(crate::backend::wayland::error::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, _f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        Ok(())
    }
}
