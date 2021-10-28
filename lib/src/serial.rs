// Copyright 2021 Oxide Computer Company

use crate::error::Error;
use std::net::SocketAddr;
use tokio_tungstenite::{
    WebSocketStream, 
    MaybeTlsStream, 
    tungstenite::Message,
};
use std::time::Duration;
use tokio::time::timeout;
use tokio::net::TcpStream;
use futures::{SinkExt, StreamExt};
use slog::{debug, Logger};

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
    pub state: State,
    log: Logger,
}

impl SerialCommander {

    pub fn new(
        addr: SocketAddr,
        instance: String,
        log: Logger
    ) -> SerialCommander {

        SerialCommander{
            addr,
            instance,
            log,
            state: State::Empty,
        }

    }

    pub async fn connect(&mut self) 
    -> Result<WebSocketStream<MaybeTlsStream<TcpStream>> , Error> {

        debug!(self.log, "sc: connecting");

        self.state = State::Connecting;
        let path = format!("ws://{}/instances/{}/serial", self.addr, self.instance);
        let (ws, _) = tokio_tungstenite::connect_async(path).await?;
        Ok(ws)

    }

    pub async fn start(&mut self) -> Result<(), Error> {

        debug!(self.log, "sc: starting");

        let mut ws = self.connect().await?;
        self.wait_for_prompt(&mut ws).await?;
        self.login(&mut ws).await?;

        Ok(())

    }

    async fn wait_for_prompt(&mut self, ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>)
    -> Result<(), Error>{

        debug!(self.log, "sc: waiting for prompt");

        // TODO hardcode
        let prompt = b"login:";
        let mut i = 0;

        loop {
            match ws.next().await  {
                Some(Ok(Message::Binary(input))) => {
                    // look for login prompt, possibly across messsages
                    for x in input {
                        if x == prompt[i] {
                            i += 1;
                            if i == prompt.len() - 1 {
                                debug!(self.log, "sc: prompt detected");
                                return Ok(());
                            }
                        }
                        else {
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

    async fn login(&mut self, ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>)
    -> Result<(), Error>{

        debug!(self.log, "sc: logging in");

        //TODO hardcode
        let mut v = Vec::from(b"root".as_slice());
        v.push(0x0du8); //<enter>
        v.push(0x0du8); //<enter>
        ws.send(Message::binary(v)).await?;

        //TODO check login

        Ok(())

    }

    pub async fn exec(
        &mut self, 
        ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
        command: String,
    ) -> Result<(), Error>{

        debug!(self.log, "sc: executing command `{}`", command);

        let mut v = Vec::from(command.as_bytes());
        v.push(0x0du8); //<enter>
        ws.send(Message::binary(v)).await?;

        Ok(())

    }

    pub async fn drain(
        &mut self, 
        ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
        command: String,
    ) -> Result<(), Error>{

        debug!(self.log, "sc: draining stream `{}`", command);

        loop {
            match timeout(Duration::from_millis(100), ws.next()).await {
                Ok(_) => { /* do something with data? */ }
                Err(_) => { break; }
            }
        }

        Ok(())

    }
}
