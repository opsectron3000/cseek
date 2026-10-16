//! Peer messages exchanged over "P" connections (`u32` length, `u32` code).

use crate::{frame, zlib_compress, zlib_decompress, DirEntry, FileEntry, Reader, Result, Writer};

pub mod code {
    pub const SHARED_FILE_LIST_REQUEST: u32 = 4;
    pub const SHARED_FILE_LIST_RESPONSE: u32 = 5;
    pub const FILE_SEARCH_RESPONSE: u32 = 9;
    pub const USER_INFO_REQUEST: u32 = 15;
    pub const USER_INFO_RESPONSE: u32 = 16;
    pub const FOLDER_CONTENTS_REQUEST: u32 = 36;
    pub const FOLDER_CONTENTS_RESPONSE: u32 = 37;
    pub const TRANSFER_REQUEST: u32 = 40;
    pub const TRANSFER_RESPONSE: u32 = 41;
    pub const QUEUE_UPLOAD: u32 = 43;
    pub const PLACE_IN_QUEUE_RESPONSE: u32 = 44;
    pub const UPLOAD_FAILED: u32 = 46;
    pub const UPLOAD_DENIED: u32 = 50;
    pub const PLACE_IN_QUEUE_REQUEST: u32 = 51;
    pub const UPLOAD_QUEUE_NOTIFICATION: u32 = 52;
}

pub const DIRECTION_DOWNLOAD: u32 = 0;
pub const DIRECTION_UPLOAD: u32 = 1;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SearchResponse {
    pub username: String,
    pub token: u32,
    pub files: Vec<FileEntry>,
    pub slot_free: bool,
    pub avg_speed: u32,
    pub queue_length: u32,
    pub private_files: Vec<FileEntry>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct UserInfo {
    pub description: String,
    pub picture: Option<Vec<u8>>,
    pub total_uploads: u32,
    pub queue_size: u32,
    pub slots_free: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PeerMessage {
    SharedFileListRequest,
    SharedFileListResponse { dirs: Vec<DirEntry>, private_dirs: Vec<DirEntry> },
    FileSearchResponse(SearchResponse),
    UserInfoRequest,
    UserInfoResponse(UserInfo),
    FolderContentsRequest { token: u32, folder: String },
    FolderContentsResponse { token: u32, folder: String, dirs: Vec<DirEntry> },
    TransferRequest { direction: u32, token: u32, filename: String, size: Option<u64> },
    TransferResponse { token: u32, allowed: bool, size: Option<u64>, reason: Option<String> },
    QueueUpload { filename: String },
    PlaceInQueueResponse { filename: String, place: u32 },
    UploadFailed { filename: String },
    UploadDenied { filename: String, reason: String },
    PlaceInQueueRequest { filename: String },
    UploadQueueNotification,
    Unknown { code: u32 },
}

impl PeerMessage {
    pub fn encode(&self) -> Vec<u8> {
        use PeerMessage::*;
        let mut w = Writer::new();
        match self {
            SharedFileListRequest => {
                w.u32(code::SHARED_FILE_LIST_REQUEST);
            }
            SharedFileListResponse { dirs, private_dirs } => {
                w.u32(code::SHARED_FILE_LIST_RESPONSE);
                let mut b = Writer::new();
                b.u32(dirs.len() as u32);
                dirs.iter().for_each(|d| d.write(&mut b));
                b.u32(0);
                b.u32(private_dirs.len() as u32);
                private_dirs.iter().for_each(|d| d.write(&mut b));
                w.raw(&zlib_compress(&b.buf));
            }
            FileSearchResponse(s) => {
                w.u32(code::FILE_SEARCH_RESPONSE);
                let mut b = Writer::new();
                b.str(&s.username).u32(s.token).u32(s.files.len() as u32);
                s.files.iter().for_each(|f| f.write(&mut b));
                b.bool(s.slot_free).u32(s.avg_speed).u32(s.queue_length).u32(0);
                b.u32(s.private_files.len() as u32);
                s.private_files.iter().for_each(|f| f.write(&mut b));
                w.raw(&zlib_compress(&b.buf));
            }
            UserInfoRequest => {
                w.u32(code::USER_INFO_REQUEST);
            }
            UserInfoResponse(i) => {
                w.u32(code::USER_INFO_RESPONSE).str(&i.description);
                match &i.picture {
                    Some(p) => w.bool(true).bytes(p),
                    None => w.bool(false),
                };
                w.u32(i.total_uploads).u32(i.queue_size).bool(i.slots_free).u32(0);
            }
            FolderContentsRequest { token, folder } => {
                w.u32(code::FOLDER_CONTENTS_REQUEST).u32(*token).str(folder);
            }
            FolderContentsResponse { token, folder, dirs } => {
                w.u32(code::FOLDER_CONTENTS_RESPONSE);
                let mut b = Writer::new();
                b.u32(*token).str(folder).u32(dirs.len() as u32);
                dirs.iter().for_each(|d| d.write(&mut b));
                w.raw(&zlib_compress(&b.buf));
            }
            TransferRequest { direction, token, filename, size } => {
                w.u32(code::TRANSFER_REQUEST).u32(*direction).u32(*token).str(filename);
                if *direction == DIRECTION_UPLOAD {
                    w.u64(size.unwrap_or(0));
                }
            }
            TransferResponse { token, allowed, size, reason } => {
                w.u32(code::TRANSFER_RESPONSE).u32(*token).bool(*allowed);
                if *allowed {
                    if let Some(s) = size {
                        w.u64(*s);
                    }
                } else {
                    w.str(reason.as_deref().unwrap_or("Cancelled"));
                }
            }
            QueueUpload { filename } => {
                w.u32(code::QUEUE_UPLOAD).str(filename);
            }
            PlaceInQueueResponse { filename, place } => {
                w.u32(code::PLACE_IN_QUEUE_RESPONSE).str(filename).u32(*place);
            }
            UploadFailed { filename } => {
                w.u32(code::UPLOAD_FAILED).str(filename);
            }
            UploadDenied { filename, reason } => {
                w.u32(code::UPLOAD_DENIED).str(filename).str(reason);
            }
            PlaceInQueueRequest { filename } => {
                w.u32(code::PLACE_IN_QUEUE_REQUEST).str(filename);
            }
            UploadQueueNotification => {
                w.u32(code::UPLOAD_QUEUE_NOTIFICATION);
            }
            Unknown { code } => {
                w.u32(*code);
            }
        }
        frame(&w.buf)
    }

    pub fn decode(payload: &[u8]) -> Result<Self> {
        use PeerMessage::*;
        let mut r = Reader::new(payload);
        let c = r.u32()?;
        let r = &mut r;
        Ok(match c {
            code::SHARED_FILE_LIST_REQUEST => SharedFileListRequest,
            code::SHARED_FILE_LIST_RESPONSE => {
                let data = zlib_decompress(r.rest())?;
                let mut r = Reader::new(&data);
                let dirs = r.list(DirEntry::read)?;
                let mut private_dirs = Vec::new();
                if r.remaining() >= 4 {
                    let _unknown = r.u32()?;
                    if r.remaining() >= 4 {
                        private_dirs = r.list(DirEntry::read).unwrap_or_default();
                    }
                }
                SharedFileListResponse { dirs, private_dirs }
            }
            code::FILE_SEARCH_RESPONSE => {
                let data = zlib_decompress(r.rest())?;
                let mut r = Reader::new(&data);
                let username = r.str()?;
                let token = r.u32()?;
                let files = r.list(FileEntry::read)?;
                let slot_free = r.bool()?;
                let avg_speed = r.u32()?;
                let queue_length = r.u32()?;
                let mut private_files = Vec::new();
                if r.remaining() >= 4 {
                    let _unknown = r.u32()?;
                    if r.remaining() >= 4 {
                        private_files = r.list(FileEntry::read).unwrap_or_default();
                    }
                }
                FileSearchResponse(SearchResponse {
                    username,
                    token,
                    files,
                    slot_free,
                    avg_speed,
                    queue_length,
                    private_files,
                })
            }
            code::USER_INFO_REQUEST => UserInfoRequest,
            code::USER_INFO_RESPONSE => {
                let description = r.str()?;
                let picture = if r.bool()? { Some(r.bytes()?) } else { None };
                let total_uploads = r.u32()?;
                let queue_size = r.u32()?;
                let slots_free = r.bool()?;
                UserInfoResponse(UserInfo { description, picture, total_uploads, queue_size, slots_free })
            }
            code::FOLDER_CONTENTS_REQUEST => FolderContentsRequest { token: r.u32()?, folder: r.str()? },
            code::FOLDER_CONTENTS_RESPONSE => {
                let data = zlib_decompress(r.rest())?;
                let mut r = Reader::new(&data);
                let token = r.u32()?;
                let folder = r.str()?;
                let dirs = r.list(DirEntry::read)?;
                FolderContentsResponse { token, folder, dirs }
            }
            code::TRANSFER_REQUEST => {
                let direction = r.u32()?;
                let token = r.u32()?;
                let filename = r.str()?;
                let size = if direction == DIRECTION_UPLOAD && r.remaining() >= 8 { Some(r.u64()?) } else { None };
                TransferRequest { direction, token, filename, size }
            }
            code::TRANSFER_RESPONSE => {
                let token = r.u32()?;
                let allowed = r.bool()?;
                if allowed {
                    let size = if r.remaining() >= 8 { Some(r.u64()?) } else { None };
                    TransferResponse { token, allowed, size, reason: None }
                } else {
                    let reason = if r.is_empty() { None } else { Some(r.str()?) };
                    TransferResponse { token, allowed, size: None, reason }
                }
            }
            code::QUEUE_UPLOAD => QueueUpload { filename: r.str()? },
            code::PLACE_IN_QUEUE_RESPONSE => PlaceInQueueResponse { filename: r.str()?, place: r.u32()? },
            code::UPLOAD_FAILED => UploadFailed { filename: r.str()? },
            code::UPLOAD_DENIED => UploadDenied { filename: r.str()?, reason: r.str()? },
            code::PLACE_IN_QUEUE_REQUEST => PlaceInQueueRequest { filename: r.str()? },
            code::UPLOAD_QUEUE_NOTIFICATION => UploadQueueNotification,
            code => Unknown { code },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(m: PeerMessage) {
        let f = m.encode();
        assert_eq!(u32::from_le_bytes(f[..4].try_into().unwrap()) as usize, f.len() - 4);
        assert_eq!(PeerMessage::decode(&f[4..]).unwrap(), m);
    }

    fn file(name: &str) -> FileEntry {
        FileEntry {
            filename: name.into(),
            size: 1234,
            extension: "mp3".into(),
            attributes: vec![(crate::attr::BITRATE, 320), (crate::attr::DURATION, 200)],
        }
    }

    #[test]
    fn roundtrips() {
        roundtrip(PeerMessage::SharedFileListRequest);
        roundtrip(PeerMessage::SharedFileListResponse {
            dirs: vec![DirEntry { name: "Music\\A".into(), files: vec![file("x.mp3")] }],
            private_dirs: vec![],
        });
        roundtrip(PeerMessage::FileSearchResponse(SearchResponse {
            username: "me".into(),
            token: 9,
            files: vec![file("Music\\A\\x.mp3")],
            slot_free: true,
            avg_speed: 1000,
            queue_length: 3,
            private_files: vec![],
        }));
        roundtrip(PeerMessage::UserInfoResponse(UserInfo {
            description: "hi".into(),
            picture: Some(vec![1, 2, 3]),
            total_uploads: 2,
            queue_size: 0,
            slots_free: true,
        }));
        roundtrip(PeerMessage::FolderContentsResponse {
            token: 5,
            folder: "Music\\A".into(),
            dirs: vec![DirEntry { name: "Music\\A".into(), files: vec![file("x.mp3")] }],
        });
        roundtrip(PeerMessage::TransferRequest {
            direction: DIRECTION_UPLOAD,
            token: 1,
            filename: "a\\b.mp3".into(),
            size: Some(99),
        });
        roundtrip(PeerMessage::TransferRequest { direction: DIRECTION_DOWNLOAD, token: 1, filename: "f".into(), size: None });
        roundtrip(PeerMessage::TransferResponse { token: 1, allowed: true, size: None, reason: None });
        roundtrip(PeerMessage::TransferResponse { token: 1, allowed: false, size: None, reason: Some("Queued".into()) });
        roundtrip(PeerMessage::QueueUpload { filename: "f".into() });
        roundtrip(PeerMessage::UploadDenied { filename: "f".into(), reason: "File not shared.".into() });
        roundtrip(PeerMessage::PlaceInQueueResponse { filename: "f".into(), place: 4 });
    }
}
