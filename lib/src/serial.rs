// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Copyright 2022 Oxide Computer Company

use crate::error::Error;
use futures::SinkExt;
use propolis_client::support::{InstanceSerialConsoleHelper, WSClientOffset};
use regex::Regex;
use slog::{debug, trace, warn, Logger};
use std::net::SocketAddr;
use tokio::time::timeout;
use tokio::time::{sleep, Duration};
use tokio_tungstenite::tungstenite::Message;

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
    eoc_regex: Regex,
    login_prompt_regex: Regex,
    log: Logger,
}

const EOC_DETECTOR: &str = "__FALCON_EXEC_FINISHED__";
const ENTER: u8 = 0x0d;
const USERNAME: &[u8] = "root".as_bytes();

impl SerialCommander {
    pub fn new(
        addr: SocketAddr,
        instance: String,
        name: String,
        log: Logger,
    ) -> SerialCommander {
        let eoc_regex = Regex::new(&format!("(?mR){EOC_DETECTOR}")).unwrap();
        let login_prompt_regex = Regex::new("login:").unwrap();
        SerialCommander {
            addr,
            instance,
            name,
            log,
            state: State::Empty,
            eoc_regex,
            login_prompt_regex,
        }
    }

    pub async fn connect(
        &mut self,
    ) -> Result<InstanceSerialConsoleHelper, Error> {
        self.state = State::Connecting;
        for _ in 0..30 {
            match InstanceSerialConsoleHelper::new(
                self.addr,
                WSClientOffset::MostRecent(0),
                Some(self.log.clone()),
            )
            .await
            {
                Ok(sc) => return Ok(sc),
                Err(e) => {
                    warn!(self.log, "[sc] {}: {}", self.name, e);
                    sleep(Duration::from_secs(1)).await;
                    continue;
                }
            }
        }

        // one more shot
        let sc = InstanceSerialConsoleHelper::new(
            self.addr,
            WSClientOffset::FromStart(0),
            Some(self.log.clone()),
        )
        .await
        .map_err(Error::InstanceSerialConnectError)?;

        Ok(sc)
    }

    pub async fn start(
        &mut self,
        coax_prompt: bool,
    ) -> Result<InstanceSerialConsoleHelper, Error> {
        debug!(self.log, "[sc] {}: starting", self.name);

        let mut ws = self.connect().await?;
        self.wait_for_login_prompt(&mut ws, coax_prompt).await?;
        self.login(&mut ws).await?;

        Ok(ws)
    }

    async fn wait_for_login_prompt(
        &mut self,
        ws: &mut InstanceSerialConsoleHelper,
        coax_prompt: bool,
    ) -> Result<(), Error> {
        debug!(self.log, "[sc] {} waiting for prompt", self.name);

        let timeout = None;
        if coax_prompt {
            let v = vec![ENTER, ENTER];
            ws.send(Message::binary(v)).await?;
        }
        self.drain_match(ws, timeout, self.login_prompt_regex.clone())
            .await?;
        Ok(())
    }

    pub(crate) async fn login(
        &mut self,
        ws: &mut InstanceSerialConsoleHelper,
    ) -> Result<(), Error> {
        debug!(self.log, "[sc] {}: logging in", self.name);

        let timeout = Some(10000);

        // Send username and wait for password prompt
        trace!(
            self.log,
            "[sc] {}: injecting username at expected password prompt",
            self.name
        );
        let mut v = Vec::from(USERNAME);
        v.push(ENTER);
        ws.send(Message::binary(v)).await?;

        // Some systems (such as our debian 11 image) don't take passwords.
        // In that case, we also accept a root prompt.
        self.drain_match(
            ws,
            timeout,
            Regex::new(r"Password:|root@.+#").unwrap(),
        )
        .await?;

        // Send empty password and wait for prompt
        trace!(
            self.log,
            "[sc] {}: Sending empty password after expected password prompt",
            self.name
        );
        let v = vec![ENTER];
        ws.send(Message::binary(v)).await?;
        self.drain_match(ws, timeout, Regex::new(r"root@.+#").unwrap())
            .await?;

        // Set the terminal type.  This is non-interactive; we don't
        // want a real terminal type with actual capabilities.
        trace!(self.log, "[sc] {}: Setting TERM=dumb", self.name);
        let cmd = r"export TERM=dumb";
        let mut v = Vec::from(cmd);
        v.push(ENTER);
        ws.send(Message::binary(v.clone())).await?;
        let regex = Regex::new(&format!("{cmd}.*\\n")).unwrap();
        self.drain_match(ws, timeout, regex).await?;

        // Put the terminal into raw mode.
        trace!(self.log, "[sc] {}: Setting raw terminal mode", self.name);
        let cmd = r"stty raw";
        let mut v = Vec::from(cmd);
        v.push(ENTER);
        ws.send(Message::binary(v.clone())).await?;
        let regex = Regex::new(&format!("{cmd}.*\\n")).unwrap();
        self.drain_match(ws, timeout, regex).await?;

        // Set the prompt command to allow us to detect the end of each command
        trace!(self.log, "[sc] {}: Setting PROMPT_COMMAND", self.name);
        let mut v = Vec::from(
            format!("PROMPT_COMMAND='echo {EOC_DETECTOR}'").as_bytes(),
        );
        v.push(ENTER);
        ws.send(Message::binary(v)).await?;
        // We can possibly see the PROMPT_COMMAND twice here, so we want to
        // do a special match where we only see it on the start of a line,
        // ignoring the echoing of our command by the terminal. This is why we
        // don't just use `self.eoc_regex`. We don't want this for the general
        // case, because in some cases (like our debian 11 image), we get output
        // prepended to the PROMPT_COMMAND.
        let regex = Regex::new(&format!("(?mR)^{EOC_DETECTOR}")).unwrap();
        self.drain_match(ws, timeout, regex).await?;

        Ok(())
    }

    pub(crate) async fn logout(
        &mut self,
        ws: &mut InstanceSerialConsoleHelper,
    ) -> Result<(), Error> {
        let timeout = Some(5000);
        let mut v = Vec::from(b"logout".as_slice());
        v.push(ENTER);
        ws.send(Message::binary(v)).await?;
        self.drain_match(ws, timeout, self.login_prompt_regex.clone())
            .await?;
        Ok(())
    }

    // Execute a command with a specific timeout
    pub async fn exec_timeout(
        &mut self,
        ws: &mut InstanceSerialConsoleHelper,
        cmd: String,
        timeout_ms: Option<u64>,
    ) -> Result<String, Error> {
        debug!(self.log, "[sc] {}: executing command `{}`", self.name, cmd);

        let mut v = Vec::from(cmd.as_bytes());
        v.push(ENTER);
        ws.send(Message::binary(v)).await?;

        let out = self
            .drain_match(ws, timeout_ms, self.eoc_regex.clone())
            .await?;

        // Iterate over all returned lines, stripping the first.
        // This could almost certainly be made more efficient, by perhaps never
        // adding the first line when parsing the regex.
        let lines = out.lines().skip(1);
        let mut stripped = String::new();
        for line in lines {
            stripped.push_str(line);
            stripped.push('\n');
        }
        // Remove the last `\n`
        stripped.pop();

        Ok(stripped)
    }

    // Execute a command with no timeout
    pub async fn exec(
        &mut self,
        ws: &mut InstanceSerialConsoleHelper,
        command: String,
    ) -> Result<String, Error> {
        self.exec_timeout(ws, command, None).await
    }

    /// Drain from the websocket until we match the provided regex or timeout.
    ///
    /// Return all read data up to the regex match or an error.
    pub async fn drain_match(
        &mut self,
        ws: &mut InstanceSerialConsoleHelper,
        wait_ms: Option<u64>,
        regex: Regex,
    ) -> Result<String, Error> {
        trace!(self.log, "[sc] {}: drain by matching regex", self.name);

        // Use the largest possible timeout if we don't want a timeout
        let wait_ms = wait_ms.unwrap_or(u64::MAX);

        let mut result = "".to_string();
        loop {
            match timeout(Duration::from_millis(wait_ms), ws.recv()).await {
                Ok(msg) => match msg {
                    Some(Ok(m)) => match m.process().await {
                        Ok(Message::Binary(data)) => {
                            let s = String::from_utf8_lossy(data.as_slice())
                                .to_string();
                            trace!(
                                self.log,
                                "[sc] {}: received data: {}",
                                self.name,
                                s
                            );
                            result += &s;
                            if let Some(mat) = regex.find(&result) {
                                trace!(
                                    self.log,
                                    "[sc] {}: drained: `{}`",
                                    self.name,
                                    &result
                                );
                                result.truncate(mat.start());
                                trace!(
                                    self.log,
                                    "[sc] {}: breaking on success",
                                    self.name
                                );
                                break;
                            }
                        }
                        Ok(Message::Close(..)) => {
                            trace!(
                                self.log,
                                "[sc] {}: breaking on close",
                                self.name
                            );
                            return Err(Error::Exec(format!(
                                "[sc] {}: websocket closed",
                                self.name
                            )));
                        }
                        Err(e) => {
                            trace!(
                                self.log,
                                "[sc] {}: breaking on wesocket error: {e}",
                                self.name,
                            );
                            return Err(Error::Exec(format!(
                                "[sc] {}: web socket error: {e}",
                                self.name
                            )));
                        }
                        _ => {
                            trace!(
                                self.log,
                                "[sc] {}: breaking on _",
                                self.name
                            );
                            return Err(Error::Exec(format!(
                                "[sc] {}: Unexpected websocket message",
                                self.name
                            )));
                        }
                    },
                    Some(Err(e)) => {
                        trace!(
                                self.log,
                                "[sc] {}: breaking on serial message process error: {e}",
                                self.name,
                            );
                        return Err(Error::Exec(format!(
                            "[sc] {}: serial message process error: {e}",
                            self.name
                        )));
                    }
                    None => {
                        trace!(
                            self.log,
                            "[sc] {}: breaking on none",
                            self.name
                        );
                        return Err(Error::Exec(format!(
                            "[sc] {}: stream returned no data",
                            self.name
                        )));
                    }
                },
                Err(_) => {
                    trace!(
                        self.log,
                        "[sc] {}: breaking on timeout: received {}",
                        self.name,
                        result
                    );
                    return Err(Error::Exec(format!(
                        "[sc] {}: timeout waiting for data",
                        self.name
                    )));
                }
            }
        }

        Ok(result)
    }
}
