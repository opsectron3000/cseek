//! Peer connection initialization (`u32` length, `u8` code), plus the
//! raw handshakes used on "F" (file transfer) connections.

use crate::{frame, Reader, Result, Writer};

pub const PIERCE_FIREWALL: u8 = 0;
pub const PEER_INIT: u8 = 1;

pub const CONN_PEER: &str = "P";
pub const CONN_FILE: &str = "F";
pub const CONN_DISTRIBUTED: &str = "D";

#[derive(Debug, Clone, PartialEq)]
pub enum InitMessage {
    PierceFirewall { token: u32 },
    PeerInit { username: String, conn_type: String, token: u32 },
    Unknown { code: u8 },
}

impl InitMessage {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        match self {
            InitMessage::PierceFirewall { token } => {
                w.u8(PIERCE_FIREWALL).u32(*token);
            }
            InitMessage::PeerInit { username, conn_type, token } => {
                w.u8(PEER_INIT).str(username).str(conn_type).u32(*token);
            }
            InitMessage::Unknown { code } => {
                w.u8(*code);
            }
        }
        frame(&w.buf)
    }

    pub fn decode(payload: &[u8]) -> Result<Self> {
        let mut r = Reader::new(payload);
        Ok(match r.u8()? {
            PIERCE_FIREWALL => InitMessage::PierceFirewall { token: r.u32()? },
            PEER_INIT => InitMessage::PeerInit {
                username: r.str()?,
                conn_type: r.str()?,
                token: if r.remaining() >= 4 { r.u32()? } else { 0 },
            },
            code => InitMessage::Unknown { code },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        for m in [
            InitMessage::PierceFirewall { token: 7 },
            InitMessage::PeerInit { username: "me".into(), conn_type: "F".into(), token: 0 },
        ] {
            let f = m.encode();
            assert_eq!(InitMessage::decode(&f[4..]).unwrap(), m);
        }
    }
}
