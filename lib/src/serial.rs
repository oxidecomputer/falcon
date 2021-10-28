// Copyright 2021 Oxide Computer Company

use crate::error::Error;
use std::net::SocketAddr;
use tokio_tungstenite::{
    WebSocketStream, 
    MaybeTlsStream, 
    tungstenite::Message,
};
use tokio::net::TcpStream;
use futures::{SinkExt, StreamExt};

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
}

impl SerialCommander {

    pub fn new(addr: SocketAddr, instance: String) -> SerialCommander {
        SerialCommander{
            addr,
            instance,
            state: State::Empty,
        }
    }

    pub async fn start(&mut self) -> Result<(), Error> {

        // connect to websocket
        self.state = State::Connecting;
        let path = format!("ws://{}/instances/{}/serial", self.addr, self.instance);
        let (mut ws, _) = tokio_tungstenite::connect_async(path).await?;

        self.wait_for_prompt(&mut ws).await?;
        self.login(&mut ws).await?;

        Ok(())

    }

    async fn wait_for_prompt(&mut self, ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>)
    -> Result<(), Error>{

        // TODO hardcode
        let prompt = [
            'l' as u8,
            'o' as u8,
            'g' as u8,
            'i' as u8,
            'n' as u8,
            ':' as u8,
        ];
        let mut i = 0;

        loop {
            match ws.next().await  {
                Some(Ok(Message::Binary(input))) => {
                    // look for login prompt, possibly across messsages
                    for x in input {
                        if x == prompt[i] {
                            i += 1;
                            if i == prompt.len() - 1 {
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

        ws.send(Message::binary(vec![
                b'r', b'o', b'o', b't', 0x0du8, //root<enter>
                0x0du8, //<enter>
        ])).await?;

        Ok(())

    }

    async fn exec(
        &mut self, 
        ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
        command: String,
    ) -> Result<(), Error>{

        let mut v = Vec::new();
        v.copy_from_slice(command.as_bytes());
        v.push(0x0du8); //<enter>
        ws.send(Message::binary(v)).await?;

        Ok(())

    }
}
