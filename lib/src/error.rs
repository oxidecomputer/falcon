// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Copyright 2022 Oxide Computer Company

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
    FUtf8(#[from] std::string::FromUtf8Error),
    #[error("exec: {0}")]
    Exec(String),
    QueryError(#[from] smf::QueryError),
    #[error("path: {0}")]
    PathError(String),
    #[error("wrap: {0}")]
    Wrap(String),
    #[error("netadm: {0}")]
    Libnet(#[from] libnet::Error),
    #[error("cli: {0}")]
    Cli(String),
    RonSpan(#[from] ron::error::SpannedError),
    Ron(#[from] ron::Error),
    TomL(#[from] toml::ser::Error),
    AddrParse(#[from] std::net::AddrParseError),
    Propolis(#[from] propolis_client::Error),
    PropolisTypes(
        #[from] propolis_client::Error<propolis_client::types::Error>,
    ),
    IntParse(#[from] std::num::ParseIntError),
    TryIntParse(#[from] std::num::TryFromIntError),
    WsError(#[from] tokio_tungstenite::tungstenite::Error),
    Anyhow(#[from] anyhow::Error),
    Uuid(#[from] uuid::Error),
    #[error("no ports available")]
    NoPorts,
    Zfs(String),
    InstanceSerialConnectError(tokio_tungstenite::tungstenite::Error),
}
