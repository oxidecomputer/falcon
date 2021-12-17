// Copyright 2021 Oxide Computer Company

use std::{ffi, io, str};
use thiserror::Error;

/// Error conditions that can be emitted by Falcon
#[derive(Error, Debug)]
#[error("{0}")]
pub enum Error {
    #[error("not implemented: {0}")]
    NotImplemented(String),
    #[error("not found: {0}")]
    NotFound(String),
    IO(#[from] io::Error),
    Zone(#[from] zone::ZoneError),
    FFI(#[from] ffi::NulError),
    Utf8(#[from] str::Utf8Error),
    #[error("exec: {0}")]
    Exec(String),
    QueryError(#[from] smf::QueryError),
    #[error("path: {0}")]
    PathError(String),
    #[error("wrap: {0}")]
    Wrap(String),
    #[error("netadm: {0}")]
    Netadm(#[from] netadm_sys::Error),
    #[error("cli: {0}")]
    Cli(String),
    Ron(#[from] ron::Error),
    TomL(#[from] toml::ser::Error),
    AddrParse(#[from] std::net::AddrParseError),
    Propolis(#[from] propolis_client::Error),
    IntParse(#[from] std::num::ParseIntError),
    WsError(#[from] tokio_tungstenite::tungstenite::Error),
    Anyhow(#[from] anyhow::Error),
}
