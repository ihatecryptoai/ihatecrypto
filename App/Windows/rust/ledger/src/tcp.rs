//! Speculos, Ledger's emulator, over its APDU socket: a 4-byte big-endian length and the APDU; the reply is a 4-byte
//! length, the data and the status word. For tests and live runs on a stand-in device; the app talks to a real one.

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::{Exchanged, LedgerError, Transport};

pub struct Speculos {
    pub host: String,
    pub port: u16,
}

impl Default for Speculos {
    fn default() -> Self { Speculos { host: "127.0.0.1".into(), port: 9999 } }
}

impl Transport for Speculos {
    fn exchange(&self, apdu: Vec<u8>, timeout: Duration) -> Exchanged<'_> {
        Box::pin(async move {
            let at = format!("{}:{}", self.host, self.port);
            let mut s = TcpStream::connect(&at).await
                .map_err(|_| LedgerError::new(format!("The Ledger emulator isn't running on {}:{}.", self.host, self.port)))?;
            let closed = |_| LedgerError::new("The emulator closed the connection.");
            s.write_all(&[(apdu.len() as u32).to_be_bytes().as_slice(), &apdu].concat()).await.map_err(closed)?;
            let answer = async {
                let length = s.read_u32().await.ok()? as usize;
                let mut reply = vec![0u8; length + 2];
                s.read_exact(&mut reply).await.ok()?;
                Some(reply)
            };
            tokio::time::timeout(timeout, answer).await.ok().flatten().ok_or_else(|| LedgerError::new("No answer from the emulator."))
        })
    }
}
