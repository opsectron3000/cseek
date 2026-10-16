//! Soulseek wire protocol.
//!
//! Every message on the wire is framed as `u32 length` followed by `length` bytes.
//! Server and peer messages carry a `u32` code; peer-init and distributed messages a `u8` code.
//! All integers are little-endian; strings are `u32 length` + bytes.

pub mod distributed;
pub mod init;
pub mod peer;
pub mod server;

use std::io::{Read, Write as _};
use std::net::Ipv4Addr;

pub use distributed::DistribMessage;
pub use init::InitMessage;
pub use peer::PeerMessage;
pub use server::{ServerRequest, ServerResponse};

/// Largest frame we accept; shared file lists of big users can be tens of MB.
pub const MAX_FRAME: usize = 256 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum ProtoError {
    #[error("unexpected end of message (needed {needed} bytes at offset {pos})")]
    Eof { pos: usize, needed: usize },
    #[error("decompression failed: {0}")]
    Zlib(String),
    #[error("invalid message: {0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, ProtoError>;

#[derive(Default, Debug, Clone)]
pub struct Writer {
    pub buf: Vec<u8>,
}

impl Writer {
    pub fn new() -> Self {
        Self { buf: Vec::with_capacity(64) }
    }
    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.buf.push(v);
        self
    }
    pub fn bool(&mut self, v: bool) -> &mut Self {
        self.u8(v as u8)
    }
    pub fn u16(&mut self, v: u16) -> &mut Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn i32(&mut self, v: i32) -> &mut Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn u64(&mut self, v: u64) -> &mut Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn str(&mut self, s: &str) -> &mut Self {
        self.u32(s.len() as u32);
        self.buf.extend_from_slice(s.as_bytes());
        self
    }
    pub fn bytes(&mut self, b: &[u8]) -> &mut Self {
        self.u32(b.len() as u32);
        self.buf.extend_from_slice(b);
        self
    }
    pub fn raw(&mut self, b: &[u8]) -> &mut Self {
        self.buf.extend_from_slice(b);
        self
    }
    pub fn ip(&mut self, ip: Ipv4Addr) -> &mut Self {
        self.u32(u32::from(ip))
    }
    pub fn strs<S: AsRef<str>>(&mut self, items: &[S]) -> &mut Self {
        self.u32(items.len() as u32);
        for s in items {
            self.str(s.as_ref());
        }
        self
    }
}

pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }
    pub fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }
    pub fn is_empty(&self) -> bool {
        self.remaining() == 0
    }
    pub fn rest(&mut self) -> &'a [u8] {
        let r = &self.buf[self.pos..];
        self.pos = self.buf.len();
        r
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.remaining() < n {
            return Err(ProtoError::Eof { pos: self.pos, needed: n });
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub fn bool(&mut self) -> Result<bool> {
        Ok(self.u8()? != 0)
    }
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    pub fn str(&mut self) -> Result<String> {
        let n = self.u32()? as usize;
        Ok(decode_string(self.take(n)?))
    }
    pub fn bytes(&mut self) -> Result<Vec<u8>> {
        let n = self.u32()? as usize;
        Ok(self.take(n)?.to_vec())
    }
    pub fn ip(&mut self) -> Result<Ipv4Addr> {
        Ok(Ipv4Addr::from(self.u32()?))
    }
    /// A `u32` count followed by that many items. Guards against absurd counts.
    pub fn list<T>(&mut self, mut f: impl FnMut(&mut Self) -> Result<T>) -> Result<Vec<T>> {
        let n = self.u32()? as usize;
        if n > self.remaining() {
            return Err(ProtoError::Invalid(format!("list count {n} exceeds payload")));
        }
        let mut v = Vec::with_capacity(n);
        for _ in 0..n {
            v.push(f(self)?);
        }
        Ok(v)
    }
    pub fn strs(&mut self) -> Result<Vec<String>> {
        self.list(|r| r.str())
    }
}

/// Soulseek strings are UTF-8 in practice, but older clients send Latin-1.
pub fn decode_string(b: &[u8]) -> String {
    match std::str::from_utf8(b) {
        Ok(s) => s.to_owned(),
        Err(_) => b.iter().map(|&c| c as char).collect(),
    }
}

/// Wrap a payload with a `u32` length prefix.
pub fn frame(payload: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(payload.len() + 4);
    v.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    v.extend_from_slice(payload);
    v
}

pub fn zlib_compress(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(data).expect("in-memory write");
    e.finish().expect("in-memory write")
}

pub fn zlib_decompress(data: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(data)
        .take(MAX_FRAME as u64)
        .read_to_end(&mut out)
        .map_err(|e| ProtoError::Zlib(e.to_string()))?;
    Ok(out)
}

/// File attribute codes used in search results and share lists.
pub mod attr {
    pub const BITRATE: u32 = 0;
    pub const DURATION: u32 = 1;
    pub const VBR: u32 = 2;
    pub const SAMPLE_RATE: u32 = 4;
    pub const BIT_DEPTH: u32 = 5;
}

/// A file entry as it appears in search results, share lists and folder contents.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileEntry {
    pub filename: String,
    pub size: u64,
    pub extension: String,
    pub attributes: Vec<(u32, u32)>,
}

impl FileEntry {
    pub fn attr(&self, code: u32) -> Option<u32> {
        self.attributes.iter().find(|(c, _)| *c == code).map(|(_, v)| *v)
    }
    pub(crate) fn write(&self, w: &mut Writer) {
        w.u8(1).str(&self.filename).u64(self.size).str(&self.extension);
        w.u32(self.attributes.len() as u32);
        for (c, v) in &self.attributes {
            w.u32(*c).u32(*v);
        }
    }
    pub(crate) fn read(r: &mut Reader) -> Result<Self> {
        let _code = r.u8()?;
        let filename = r.str()?;
        let size = r.u64()?;
        let extension = r.str()?;
        let attributes = r.list(|r| Ok((r.u32()?, r.u32()?)))?;
        Ok(Self { filename, size, extension, attributes })
    }
}

/// A directory with its files (file names relative to the directory).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DirEntry {
    pub name: String,
    pub files: Vec<FileEntry>,
}

impl DirEntry {
    pub(crate) fn write(&self, w: &mut Writer) {
        w.str(&self.name).u32(self.files.len() as u32);
        for f in &self.files {
            f.write(w);
        }
    }
    pub(crate) fn read(r: &mut Reader) -> Result<Self> {
        let name = r.str()?;
        let files = r.list(FileEntry::read)?;
        Ok(Self { name, files })
    }
}

/// MD5 hex digest helper for the login hash.
pub fn login_hash(username: &str, password: &str) -> String {
    format!("{:x}", md5_digest(format!("{username}{password}").as_bytes()))
}

// Tiny MD5 so the protocol crate stays dependency-light.
fn md5_digest(input: &[u8]) -> Md5Digest {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10,
        15, 21, 6, 10, 15, 21,
    ];
    let k: Vec<u32> = (0..64).map(|i| ((i as f64 + 1.0).sin().abs() * 4294967296.0) as u32).collect();
    let (mut a0, mut b0, mut c0, mut d0) = (0x67452301u32, 0xefcdab89u32, 0x98badcfeu32, 0x10325476u32);
    let mut msg = input.to_vec();
    let bit_len = (input.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_le_bytes());
    for chunk in msg.chunks(64) {
        let m: Vec<u32> = chunk.chunks(4).map(|b| u32::from_le_bytes(b.try_into().unwrap())).collect();
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | (!b & d), i),
                16..=31 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let f = f.wrapping_add(a).wrapping_add(k[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(S[i]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }
    let mut out = [0u8; 16];
    for (i, v) in [a0, b0, c0, d0].iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    Md5Digest(out)
}

struct Md5Digest([u8; 16]);
impl std::fmt::LowerHex for Md5Digest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for b in self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md5_known_vectors() {
        assert_eq!(format!("{:x}", md5_digest(b"")), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(
            format!("{:x}", md5_digest(b"The quick brown fox jumps over the lazy dog")),
            "9e107d9d372bb6826bd81d3542a419d6"
        );
    }

    #[test]
    fn ip_byte_order() {
        // Soulseek sends the address as a little-endian u32 of the big-endian value.
        let mut r = Reader::new(&[4, 3, 2, 1]);
        assert_eq!(r.ip().unwrap(), Ipv4Addr::new(1, 2, 3, 4));
        let mut w = Writer::new();
        w.ip(Ipv4Addr::new(1, 2, 3, 4));
        assert_eq!(w.buf, vec![4, 3, 2, 1]);
    }

    #[test]
    fn latin1_fallback() {
        assert_eq!(decode_string(&[0x63, 0x61, 0x66, 0xe9]), "café");
    }
}
