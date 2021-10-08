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
    IO(#[from] io::Error),
    Zone(#[from] zone::ZoneError),
    #[error("{0} {1}")]
    Dladm(String, u32),
    FFI(#[from] ffi::NulError),
    Utf8(#[from] str::Utf8Error),
    Exec(String),
    QueryError(#[from] smf::QueryError),
    PathError(String),
    Zfs(String),
    Wrap(String),
    Netadm(#[from] netadm_sys::Error),
}
