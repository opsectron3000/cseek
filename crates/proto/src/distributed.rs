//! Distributed network messages over "D" connections (`u32` length, `u8` code).

use crate::{frame, Reader, Result, Writer};

pub mod code {
    pub const PING: u8 = 0;
    pub const SEARCH: u8 = 3;
    pub const BRANCH_LEVEL: u8 = 4;
    pub const BRANCH_ROOT: u8 = 5;
    pub const CHILD_DEPTH: u8 = 7;
    pub const EMBEDDED_MESSAGE: u8 = 93;
}

#[derive(Debug, Clone, PartialEq)]
pub enum DistribMessage {
    Ping,
    Search { username: String, token: u32, query: String },
    BranchLevel { level: i32 },
    BranchRoot { root: String },
    ChildDepth { depth: u32 },
    EmbeddedMessage { distrib_code: u8, payload: Vec<u8> },
    Unknown { code: u8 },
}

impl DistribMessage {
    pub fn encode(&self) -> Vec<u8> {
        use DistribMessage::*;
        let mut w = Writer::new();
        match self {
            Ping => {
                w.u8(code::PING);
            }
            Search { username, token, query } => {
                w.u8(code::SEARCH).u32(0x31).str(username).u32(*token).str(query);
            }
            BranchLevel { level } => {
                w.u8(code::BRANCH_LEVEL).i32(*level);
            }
            BranchRoot { root } => {
                w.u8(code::BRANCH_ROOT).str(root);
            }
            ChildDepth { depth } => {
                w.u8(code::CHILD_DEPTH).u32(*depth);
            }
            EmbeddedMessage { distrib_code, payload } => {
                w.u8(code::EMBEDDED_MESSAGE).u8(*distrib_code).raw(payload);
            }
            Unknown { code } => {
                w.u8(*code);
            }
        }
        frame(&w.buf)
    }

    /// Decode a payload that starts with the `u8` code.
    pub fn decode(payload: &[u8]) -> Result<Self> {
        let mut r = Reader::new(payload);
        let c = r.u8()?;
        Self::decode_body(c, r.rest())
    }

    /// Decode given a separately supplied code (used for server-embedded messages).
    pub fn decode_body(c: u8, body: &[u8]) -> Result<Self> {
        use DistribMessage::*;
        let mut r = Reader::new(body);
        Ok(match c {
            code::PING => Ping,
            code::SEARCH => {
                let _unknown = r.u32()?;
                Search { username: r.str()?, token: r.u32()?, query: r.str()? }
            }
            code::BRANCH_LEVEL => BranchLevel { level: r.i32()? },
            code::BRANCH_ROOT => BranchRoot { root: r.str()? },
            code::CHILD_DEPTH => ChildDepth { depth: r.u32()? },
            code::EMBEDDED_MESSAGE => EmbeddedMessage { distrib_code: r.u8()?, payload: r.rest().to_vec() },
            code => Unknown { code },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_roundtrip() {
        let m = DistribMessage::Search { username: "u".into(), token: 42, query: "abc def".into() };
        let f = m.encode();
        assert_eq!(DistribMessage::decode(&f[4..]).unwrap(), m);
    }
}
