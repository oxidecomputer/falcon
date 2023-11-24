// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Copyright 2022 Oxide Computer Company

use crate::error::Error;
use futures::{SinkExt, StreamExt};
use slog::{debug, trace, warn, Logger};
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio::time::{sleep, Duration};
use tokio_tungstenite::{
    connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream,
};

pub enum State {
    Empty,
    Connecting,
    WaitingForPrompt,
    Ready,
    Executing,
}

pub struct SerialCommander {
    pub addr: SocketAddr,
    pub instance: String,
    pub name: String,
    pub state: State,
    log: Logger,
}

const EOC_DETECTOR: &str = "__FALCON_EXEC_FINISHED__";

impl SerialCommander {
    pub fn new(
        addr: SocketAddr,
        instance: String,
        name: String,
        log: Logger,
    ) -> SerialCommander {
        SerialCommander {
            addr,
            instance,
            name,
            log,
            state: State::Empty,
        }
    }

    pub async fn connect(
        &mut self,
    ) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>, Error> {
        self.state = State::Connecting;
        let path = format!("ws://{}/instance/serial", self.addr);

        debug!(self.log, "[sc] {}: connecting to {}", self.name, path);

        for _ in 0..30 {
            match connect_async(path.clone()).await {
                Ok((ws, _)) => return Ok(ws),
                Err(e) => {
                    warn!(self.log, "[sc] {}: {}", self.name, e);
                    sleep(Duration::from_secs(1)).await;
                    continue;
                }
            }
        }
        // one more shot
        let (ws, _) = connect_async(path).await?;
        Ok(ws)
    }

    pub async fn start(
        &mut self,
        coax_prompt: bool,
    ) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>, Error> {
        debug!(self.log, "[sc] {}: starting", self.name);

        let mut ws = self.connect().await?;
        self.wait_for_prompt(&mut ws, coax_prompt).await?;
        self.login(&mut ws).await?;

        Ok(ws)
    }

    async fn wait_for_prompt(
        &mut self,
        ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
        coax_prompt: bool,
    ) -> Result<(), Error> {
        debug!(self.log, "[sc] {} waiting for prompt", self.name);

        // TODO hardcode
        let prompt = b"login:";
        let mut i = 0;

        loop {
            if coax_prompt {
                let v = vec![0x0du8, 0x0du8]; //<enter><enter>
                ws.send(Message::binary(v)).await?;
            }
            match ws.next().await {
                Some(Ok(Message::Binary(input))) => {
                    // look for login prompt, possibly across messsages
                    for x in input {
                        if x == prompt[i] {
                            i += 1;
                            if i == prompt.len() {
                                debug!(
                                    self.log,
                                    "[sc] {} prompt detected", self.name
                                );
                                return Ok(());
                            }
                        } else {
                            i = 0;
                        }
                    }
                }
                Some(Ok(Message::Close(..))) | None => break,
                _ => continue,
            }
        }

        Ok(())
    }

    pub(crate) async fn login(
        &mut self,
        ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    ) -> Result<(), Error> {
        debug!(self.log, "[sc] {}: logging in", self.name);

        //TODO hardcode
        let mut v = Vec::from(b"root".as_slice());
        v.push(0x0du8); //<enter>
        ws.send(Message::binary(v)).await?;
        self.drain(ws, 1000).await?;

        let v = vec![0x0du8];
        ws.send(Message::binary(v)).await?;
        self.drain(ws, 1000).await?;

        let mut v = Vec::from(b"export TERM=xterm".as_slice());
        v.push(0x0du8); //<enter>
        ws.send(Message::binary(v)).await?;
        self.drain(ws, 1000).await?;

        let mut v = Vec::from(
            b"PROMPT_COMMAND='echo __FALCON_EXEC_FINISHED__'".as_slice(),
        );
        v.push(0x0du8); //<enter>
        ws.send(Message::binary(v)).await?;
        self.drain(ws, 1000).await?;

        //TODO check login
        Ok(())
    }

    pub(crate) async fn logout(
        &mut self,
        ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    ) -> Result<(), Error> {
        let mut v = Vec::from(b"logout".as_slice());
        v.push(0x0du8); //<enter>
        ws.send(Message::binary(v)).await?;
        self.drain(ws, 1000).await?;

        Ok(())
    }

    //TODO this could be much more robust than it is. It would be good to have
    //some sort of terminal state machine that consumes terminal input and does
    //the right thing rather than looking for potentially problematic characters
    //in the output ad-hoc.
    pub async fn exec(
        &mut self,
        ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
        command: String,
    ) -> Result<String, Error> {
        debug!(
            self.log,
            "[sc] {}: executing command `{}`", self.name, command
        );

        let cmd = command.to_string();

        let v = Vec::from(cmd.as_bytes());
        ws.send(Message::binary(v)).await?;
        self.drain_detector(ws, cmd.as_bytes()).await?;
        ws.send(Message::binary(vec![0x0du8])).await?; //<enter>
        let s = self.drain_detector(ws, EOC_DETECTOR.as_bytes()).await?;
        // remove paste mode terminal characters if present
        let s = s.replace("\u{1b}[?2004l", "");
        let s = s.replace("\u{1b}[?2004h", "");
        // remove the end oof command detector
        let s = s.replace(EOC_DETECTOR, "");
        let mut s = s.trim();
        //TODO assumes there will be a newline after the command, which is not
        //always the case
        if let Some(i) = s.rfind("\r\n") {
            s = &s[..i];
        }
        //let s = s.replace(|c: char| c.is_control(), "");
        Ok(s.trim().to_string())
    }

    pub async fn drain_detector(
        &mut self,
        ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
        detector: &[u8],
    ) -> Result<String, Error> {
        trace!(self.log, "[sc] {}: draining stream", self.name);

        let mut result = "".to_string();
        let mut i = 0;

        loop {
            match ws.next().await {
                Some(Ok(Message::Binary(data))) => {
                    for x in &data {
                        if *x == detector[i] {
                            i += 1;
                            if i == detector.len() - 1 {
                                let s =
                                    String::from_utf8_lossy(data.as_slice())
                                        .to_string();
                                result += &s;
                                debug!(
                                    self.log,
                                    "[sc] {}: detector detected", self.name
                                );
                                trace!(
                                    self.log,
                                    "[sc] {}: drained: `{}`",
                                    self.name,
                                    &result
                                );
                                self.drain(ws, 500).await?;
                                return Ok(result);
                            }
                        } else {
                            i = 0;
                        }
                    }
                    let s =
                        String::from_utf8_lossy(data.as_slice()).to_string();
                    result += &s;
                    trace!(
                        self.log,
                        "[sc] {}: partial result `{}`",
                        self.name,
                        &result
                    );
                }
                Some(Ok(Message::Close(..))) => {
                    trace!(self.log, "[sc] {}: breaking on close", self.name);
                    break;
                }
                None => {
                    trace!(self.log, "[sc] {}: breaking on none", self.name);
                    break;
                }
                _ => {
                    trace!(self.log, "[sc] {}: breaking on _", self.name);
                    break;
                }
            }
        }

        trace!(self.log, "[sc] {}: drained: `{}`", self.name, &result);

        Ok(result.to_string())
    }

    pub async fn drain(
        &mut self,
        ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
        wait: u64,
    ) -> Result<String, Error> {
        trace!(self.log, "[sc] {}: draining stream", self.name);

        let mut result = "".to_string();

        loop {
            match timeout(Duration::from_millis(wait), ws.next()).await {
                Ok(msg) => match msg {
                    Some(Ok(Message::Binary(data))) => {
                        let s = String::from_utf8_lossy(data.as_slice())
                            .to_string();
                        result += &s;
                    }
                    Some(Ok(Message::Close(..))) => {
                        trace!(
                            self.log,
                            "[sc] {}: breaking on close",
                            self.name
                        );
                        break;
                    }
                    None => {
                        trace!(
                            self.log,
                            "[sc] {}: breaking on none",
                            self.name
                        );
                        break;
                    }
                    _ => {
                        trace!(self.log, "[sc] {}: breaking on _", self.name);
                        break;
                    }
                },
                Err(_) => {
                    trace!(self.log, "[sc] {}: breaking on timeout", self.name);
                    break;
                }
            }
        }

        trace!(self.log, "[sc] {}: drained: `{}`", self.name, &result);

        Ok(result)
    }
}
