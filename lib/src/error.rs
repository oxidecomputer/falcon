// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Copyright 2022 Oxide Computer Company

use std::{io, str};
use thiserror::Error;

/// Error conditions that can be emitted by Falcon
#[derive(Error, Debug)]
#[error("{0}")]
pub enum Error {
    #[error("not found: {0}")]
    NotFound(String),
    IO(#[from] io::Error),
    Utf8(#[from] str::Utf8Error),
    FUtf8(#[from] std::string::FromUtf8Error),
    #[error("exec: {0}")]
    Exec(String),
    #[error("path: {0}")]
    PathError(String),
    #[error("netadm: {0}")]
    Libnet(#[from] libnet::Error),
    #[error("libnet route error: {0}")]
    LibnetRoute(#[from] libnet::route::Error),
    #[error("cli: {0}")]
    Cli(String),
    #[error("exclusive iface used multiple times: {0}")]
    ExternalNicReused(String),
    RonSpan(#[from] ron::error::SpannedError),
    Ron(#[from] ron::Error),
    AddrParse(#[from] std::net::AddrParseError),
    PropolisTypes(Box<propolis_client::Error<propolis_client::types::Error>>),
    IntParse(#[from] std::num::ParseIntError),
    WsError(Box<tokio_tungstenite::tungstenite::Error>),
    Anyhow(#[from] anyhow::Error),
    Uuid(#[from] uuid::Error),
    Zfs(String),
    #[error("default route has no interface")]
    NoInterfaceForDefaultRoute,
}

impl From<Box<tokio_tungstenite::tungstenite::Error>> for Error {
    fn from(value: Box<tokio_tungstenite::tungstenite::Error>) -> Self {
        Self::WsError(value)
    }
}

impl From<tokio_tungstenite::tungstenite::Error> for Error {
    fn from(value: tokio_tungstenite::tungstenite::Error) -> Self {
        Self::WsError(Box::new(value))
    }
}

impl From<propolis_client::Error<propolis_client::types::Error>> for Error {
    fn from(
        value: propolis_client::Error<propolis_client::types::Error>,
    ) -> Self {
        Self::PropolisTypes(Box::new(value))
    }
}
