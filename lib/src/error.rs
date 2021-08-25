// Copyright 2021 Oxide Computer Company

use std::{ffi, io, str};
use thiserror::Error;

/// Error conditions that can be emitted by Falcon
#[derive(Error, Debug)]
#[error("{0}")]
pub enum Error {
    #[error("not implemented")]
    NotImplemented,
    #[error("not found")]
    NotFound,
    IO(io::Error),
    Zone(zone::ZoneError),
    #[error("{0} {1}")]
    Dladm(String, u32),
    FFI(ffi::NulError),
    Utf8(str::Utf8Error),
    Exec(String),
    QueryError(smf::QueryError),
    PathError(String),
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Error {
        Error::IO(e)
    }
}

impl From<zone::ZoneError> for Error {
    fn from(e: zone::ZoneError) -> Error {
        Error::Zone(e)
    }
}

impl From<ffi::NulError> for Error {
    fn from(e: ffi::NulError) -> Error {
        Error::FFI(e)
    }
}

impl From<str::Utf8Error> for Error {
    fn from(e: str::Utf8Error) -> Error {
        Error::Utf8(e)
    }
}

impl From<smf::QueryError> for Error {
    fn from(e: smf::QueryError) -> Error {
        Error::QueryError(e)
    }
}
