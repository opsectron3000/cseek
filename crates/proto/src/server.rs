//! Server messages (`u32` length, `u32` code).

use crate::{frame, Reader, Result, Writer};
use std::net::Ipv4Addr;

pub mod code {
    pub const LOGIN: u32 = 1;
    pub const SET_WAIT_PORT: u32 = 2;
    pub const GET_PEER_ADDRESS: u32 = 3;
    pub const WATCH_USER: u32 = 5;
    pub const UNWATCH_USER: u32 = 6;
    pub const GET_USER_STATUS: u32 = 7;
    pub const SAY_CHATROOM: u32 = 13;
    pub const JOIN_ROOM: u32 = 14;
    pub const LEAVE_ROOM: u32 = 15;
    pub const USER_JOINED_ROOM: u32 = 16;
    pub const USER_LEFT_ROOM: u32 = 17;
    pub const CONNECT_TO_PEER: u32 = 18;
    pub const MESSAGE_USER: u32 = 22;
    pub const MESSAGE_ACKED: u32 = 23;
    pub const FILE_SEARCH: u32 = 26;
    pub const SET_STATUS: u32 = 28;
    pub const SERVER_PING: u32 = 32;
    pub const SHARED_FOLDERS_FILES: u32 = 35;
    pub const GET_USER_STATS: u32 = 36;
    pub const RELOGGED: u32 = 41;
    pub const USER_SEARCH: u32 = 42;
    pub const ADD_THING_I_LIKE: u32 = 51;
    pub const REMOVE_THING_I_LIKE: u32 = 52;
    pub const RECOMMENDATIONS: u32 = 54;
    pub const GLOBAL_RECOMMENDATIONS: u32 = 56;
    pub const USER_INTERESTS: u32 = 57;
    pub const ROOM_LIST: u32 = 64;
    pub const ADMIN_MESSAGE: u32 = 66;
    pub const PRIVILEGED_USERS: u32 = 69;
    pub const HAVE_NO_PARENT: u32 = 71;
    pub const PARENT_MIN_SPEED: u32 = 83;
    pub const PARENT_SPEED_RATIO: u32 = 84;
    pub const CHECK_PRIVILEGES: u32 = 92;
    pub const EMBEDDED_MESSAGE: u32 = 93;
    pub const ACCEPT_CHILDREN: u32 = 100;
    pub const POSSIBLE_PARENTS: u32 = 102;
    pub const WISHLIST_SEARCH: u32 = 103;
    pub const WISHLIST_INTERVAL: u32 = 104;
    pub const SIMILAR_USERS: u32 = 110;
    pub const ITEM_RECOMMENDATIONS: u32 = 111;
    pub const ITEM_SIMILAR_USERS: u32 = 112;
    pub const ROOM_TICKER_STATE: u32 = 113;
    pub const ROOM_TICKER_ADD: u32 = 114;
    pub const ROOM_TICKER_REMOVE: u32 = 115;
    pub const ROOM_TICKER_SET: u32 = 116;
    pub const ADD_THING_I_HATE: u32 = 117;
    pub const REMOVE_THING_I_HATE: u32 = 118;
    pub const ROOM_SEARCH: u32 = 120;
    pub const SEND_UPLOAD_SPEED: u32 = 121;
    pub const BRANCH_LEVEL: u32 = 126;
    pub const BRANCH_ROOT: u32 = 127;
    pub const CHILD_DEPTH: u32 = 129;
    pub const RESET_DISTRIBUTED: u32 = 130;
    pub const MESSAGE_USERS: u32 = 149;
    pub const EXCLUDED_SEARCH_PHRASES: u32 = 160;
    pub const CANT_CONNECT_TO_PEER: u32 = 1001;
    pub const CANT_CREATE_ROOM: u32 = 1003;
}

pub const CLIENT_VERSION: u32 = 160;
pub const CLIENT_MINOR_VERSION: u32 = 3;

/// Messages the client sends to the server.
#[derive(Debug, Clone, PartialEq)]
pub enum ServerRequest {
    Login { username: String, password: String },
    SetWaitPort { port: u32 },
    GetPeerAddress { username: String },
    WatchUser { username: String },
    UnwatchUser { username: String },
    GetUserStatus { username: String },
    SayChatroom { room: String, message: String },
    JoinRoom { room: String },
    LeaveRoom { room: String },
    ConnectToPeer { token: u32, username: String, conn_type: String },
    MessageUser { username: String, message: String },
    MessageAcked { id: u32 },
    FileSearch { token: u32, query: String },
    WishlistSearch { token: u32, query: String },
    UserSearch { username: String, token: u32, query: String },
    RoomSearch { room: String, token: u32, query: String },
    SetStatus { status: u32 },
    ServerPing,
    SharedFoldersFiles { dirs: u32, files: u32 },
    GetUserStats { username: String },
    AddThingILike { item: String },
    RemoveThingILike { item: String },
    AddThingIHate { item: String },
    RemoveThingIHate { item: String },
    GetRecommendations,
    GetGlobalRecommendations,
    GetUserInterests { username: String },
    RoomList,
    HaveNoParent { value: bool },
    CheckPrivileges,
    AcceptChildren { value: bool },
    GetSimilarUsers,
    GetItemRecommendations { item: String },
    GetItemSimilarUsers { item: String },
    RoomTickerSet { room: String, ticker: String },
    SendUploadSpeed { speed: u32 },
    BranchLevel { level: u32 },
    BranchRoot { root: String },
    ChildDepth { depth: u32 },
    CantConnectToPeer { token: u32, username: String },
}

impl ServerRequest {
    pub fn code(&self) -> u32 {
        use code::*;
        use ServerRequest::*;
        match self {
            Login { .. } => LOGIN,
            SetWaitPort { .. } => SET_WAIT_PORT,
            GetPeerAddress { .. } => GET_PEER_ADDRESS,
            WatchUser { .. } => WATCH_USER,
            UnwatchUser { .. } => UNWATCH_USER,
            GetUserStatus { .. } => GET_USER_STATUS,
            SayChatroom { .. } => SAY_CHATROOM,
            JoinRoom { .. } => JOIN_ROOM,
            LeaveRoom { .. } => LEAVE_ROOM,
            ConnectToPeer { .. } => CONNECT_TO_PEER,
            MessageUser { .. } => MESSAGE_USER,
            MessageAcked { .. } => MESSAGE_ACKED,
            FileSearch { .. } => FILE_SEARCH,
            WishlistSearch { .. } => WISHLIST_SEARCH,
            UserSearch { .. } => USER_SEARCH,
            RoomSearch { .. } => ROOM_SEARCH,
            SetStatus { .. } => SET_STATUS,
            ServerPing => SERVER_PING,
            SharedFoldersFiles { .. } => SHARED_FOLDERS_FILES,
            GetUserStats { .. } => GET_USER_STATS,
            AddThingILike { .. } => ADD_THING_I_LIKE,
            RemoveThingILike { .. } => REMOVE_THING_I_LIKE,
            AddThingIHate { .. } => ADD_THING_I_HATE,
            RemoveThingIHate { .. } => REMOVE_THING_I_HATE,
            GetRecommendations => RECOMMENDATIONS,
            GetGlobalRecommendations => GLOBAL_RECOMMENDATIONS,
            GetUserInterests { .. } => USER_INTERESTS,
            RoomList => ROOM_LIST,
            HaveNoParent { .. } => HAVE_NO_PARENT,
            CheckPrivileges => CHECK_PRIVILEGES,
            AcceptChildren { .. } => ACCEPT_CHILDREN,
            GetSimilarUsers => SIMILAR_USERS,
            GetItemRecommendations { .. } => ITEM_RECOMMENDATIONS,
            GetItemSimilarUsers { .. } => ITEM_SIMILAR_USERS,
            RoomTickerSet { .. } => ROOM_TICKER_SET,
            SendUploadSpeed { .. } => SEND_UPLOAD_SPEED,
            BranchLevel { .. } => BRANCH_LEVEL,
            BranchRoot { .. } => BRANCH_ROOT,
            ChildDepth { .. } => CHILD_DEPTH,
            CantConnectToPeer { .. } => CANT_CONNECT_TO_PEER,
        }
    }

    /// Encode as a complete frame (length + code + payload).
    pub fn encode(&self) -> Vec<u8> {
        use ServerRequest::*;
        let mut w = Writer::new();
        w.u32(self.code());
        match self {
            Login { username, password } => {
                w.str(username)
                    .str(password)
                    .u32(CLIENT_VERSION)
                    .str(&crate::login_hash(username, password))
                    .u32(CLIENT_MINOR_VERSION);
            }
            SetWaitPort { port } => {
                w.u32(*port);
            }
            GetPeerAddress { username }
            | WatchUser { username }
            | UnwatchUser { username }
            | GetUserStatus { username }
            | GetUserStats { username }
            | GetUserInterests { username } => {
                w.str(username);
            }
            SayChatroom { room, message } => {
                w.str(room).str(message);
            }
            JoinRoom { room } => {
                w.str(room).u32(0);
            }
            LeaveRoom { room } => {
                w.str(room);
            }
            ConnectToPeer { token, username, conn_type } => {
                w.u32(*token).str(username).str(conn_type);
            }
            MessageUser { username, message } => {
                w.str(username).str(message);
            }
            MessageAcked { id } => {
                w.u32(*id);
            }
            FileSearch { token, query } | WishlistSearch { token, query } => {
                w.u32(*token).str(query);
            }
            UserSearch { username, token, query } => {
                w.str(username).u32(*token).str(query);
            }
            RoomSearch { room, token, query } => {
                w.str(room).u32(*token).str(query);
            }
            SetStatus { status } => {
                w.u32(*status);
            }
            SharedFoldersFiles { dirs, files } => {
                w.u32(*dirs).u32(*files);
            }
            AddThingILike { item }
            | RemoveThingILike { item }
            | AddThingIHate { item }
            | RemoveThingIHate { item }
            | GetItemRecommendations { item }
            | GetItemSimilarUsers { item } => {
                w.str(item);
            }
            HaveNoParent { value } | AcceptChildren { value } => {
                w.bool(*value);
            }
            RoomTickerSet { room, ticker } => {
                w.str(room).str(ticker);
            }
            SendUploadSpeed { speed } => {
                w.u32(*speed);
            }
            BranchLevel { level } => {
                w.u32(*level);
            }
            BranchRoot { root } => {
                w.str(root);
            }
            ChildDepth { depth } => {
                w.u32(*depth);
            }
            CantConnectToPeer { token, username } => {
                w.u32(*token).str(username);
            }
            ServerPing | GetRecommendations | GetGlobalRecommendations | RoomList | CheckPrivileges
            | GetSimilarUsers => {}
        }
        frame(&w.buf)
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct UserStats {
    pub avg_speed: u32,
    pub upload_count: u32,
    pub files: u32,
    pub dirs: u32,
}

impl UserStats {
    fn read(r: &mut Reader) -> Result<Self> {
        let avg_speed = r.u32()?;
        let upload_count = r.u32()?;
        let _unknown = r.u32()?;
        let files = r.u32()?;
        let dirs = r.u32()?;
        Ok(Self { avg_speed, upload_count, files, dirs })
    }
    pub fn write(&self, w: &mut Writer) {
        w.u32(self.avg_speed).u32(self.upload_count).u32(0).u32(self.files).u32(self.dirs);
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct RoomUserData {
    pub username: String,
    pub status: u32,
    pub stats: UserStats,
    pub slots_full: u32,
    pub country: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ServerResponse {
    LoginSuccess { greeting: String, ip: Ipv4Addr, is_supporter: bool },
    LoginFailure { reason: String },
    PeerAddress { username: String, ip: Ipv4Addr, port: u32 },
    WatchUser { username: String, exists: bool, status: u32, stats: UserStats, country: Option<String> },
    UserStatus { username: String, status: u32, privileged: bool },
    SayChatroom { room: String, username: String, message: String },
    JoinRoom { room: String, users: Vec<RoomUserData>, owner: Option<String>, operators: Vec<String> },
    LeaveRoom { room: String },
    UserJoinedRoom { room: String, user: RoomUserData },
    UserLeftRoom { room: String, username: String },
    ConnectToPeer { username: String, conn_type: String, ip: Ipv4Addr, port: u32, token: u32, privileged: bool },
    MessageUser { id: u32, timestamp: u32, username: String, message: String, is_new: bool },
    FileSearch { username: String, token: u32, query: String },
    UserStats { username: String, stats: UserStats },
    Relogged,
    Recommendations { recommendations: Vec<(String, i32)>, unrecommendations: Vec<(String, i32)> },
    GlobalRecommendations { recommendations: Vec<(String, i32)>, unrecommendations: Vec<(String, i32)> },
    UserInterests { username: String, likes: Vec<String>, hates: Vec<String> },
    RoomList { rooms: Vec<(String, u32)>, private_rooms: Vec<(String, u32)> },
    AdminMessage { message: String },
    PrivilegedUsers { users: Vec<String> },
    ParentMinSpeed { speed: u32 },
    ParentSpeedRatio { ratio: u32 },
    CheckPrivileges { seconds_left: u32 },
    EmbeddedMessage { distrib_code: u8, payload: Vec<u8> },
    PossibleParents { parents: Vec<(String, Ipv4Addr, u32)> },
    WishlistInterval { seconds: u32 },
    SimilarUsers { users: Vec<(String, u32)> },
    ItemRecommendations { item: String, recommendations: Vec<(String, i32)> },
    ItemSimilarUsers { item: String, users: Vec<String> },
    RoomTickerState { room: String, tickers: Vec<(String, String)> },
    RoomTickerAdd { room: String, username: String, ticker: String },
    RoomTickerRemove { room: String, username: String },
    ResetDistributed,
    ExcludedSearchPhrases { phrases: Vec<String> },
    CantConnectToPeer { token: u32 },
    CantCreateRoom { room: String },
    Unknown { code: u32 },
}

fn scored(r: &mut Reader) -> Result<Vec<(String, i32)>> {
    r.list(|r| Ok((r.str()?, r.i32()?)))
}

impl ServerResponse {
    /// Decode a frame payload (without the length prefix).
    pub fn decode(payload: &[u8]) -> Result<Self> {
        use code::*;
        let mut r = Reader::new(payload);
        let code = r.u32()?;
        let r = &mut r;
        Ok(match code {
            LOGIN => {
                if r.bool()? {
                    let greeting = r.str()?;
                    let ip = r.ip()?;
                    let _hash = if r.is_empty() { String::new() } else { r.str()? };
                    let is_supporter = if r.is_empty() { false } else { r.bool()? };
                    Self::LoginSuccess { greeting, ip, is_supporter }
                } else {
                    let mut reason = r.str()?;
                    if !r.is_empty() {
                        if let Ok(detail) = r.str() {
                            reason = format!("{reason}: {detail}");
                        }
                    }
                    Self::LoginFailure { reason }
                }
            }
            GET_PEER_ADDRESS => Self::PeerAddress { username: r.str()?, ip: r.ip()?, port: r.u32()? },
            WATCH_USER => {
                let username = r.str()?;
                let exists = r.bool()?;
                if !exists || r.is_empty() {
                    Self::WatchUser { username, exists, status: 0, stats: UserStats::default(), country: None }
                } else {
                    let status = r.u32()?;
                    let stats = UserStats::read(r)?;
                    let country = if r.is_empty() { None } else { Some(r.str()?) };
                    Self::WatchUser { username, exists, status, stats, country }
                }
            }
            GET_USER_STATUS => {
                let username = r.str()?;
                let status = r.u32()?;
                let privileged = if r.is_empty() { false } else { r.bool()? };
                Self::UserStatus { username, status, privileged }
            }
            SAY_CHATROOM => Self::SayChatroom { room: r.str()?, username: r.str()?, message: r.str()? },
            JOIN_ROOM => {
                let room = r.str()?;
                let names = r.strs()?;
                let statuses = r.list(|r| r.u32())?;
                let stats = r.list(UserStats::read)?;
                let slots = r.list(|r| r.u32()).unwrap_or_default();
                let countries = r.strs().unwrap_or_default();
                let (owner, operators) =
                    if r.is_empty() { (None, vec![]) } else { (r.str().ok(), r.strs().unwrap_or_default()) };
                let users = names
                    .into_iter()
                    .enumerate()
                    .map(|(i, username)| RoomUserData {
                        username,
                        status: statuses.get(i).copied().unwrap_or(2),
                        stats: stats.get(i).cloned().unwrap_or_default(),
                        slots_full: slots.get(i).copied().unwrap_or(0),
                        country: countries.get(i).cloned().filter(|c| !c.is_empty()),
                    })
                    .collect();
                Self::JoinRoom { room, users, owner, operators }
            }
            LEAVE_ROOM => Self::LeaveRoom { room: r.str()? },
            USER_JOINED_ROOM => {
                let room = r.str()?;
                let username = r.str()?;
                let status = r.u32()?;
                let stats = UserStats::read(r)?;
                let slots_full = r.u32().unwrap_or(0);
                let country = r.str().ok().filter(|c| !c.is_empty());
                Self::UserJoinedRoom { room, user: RoomUserData { username, status, stats, slots_full, country } }
            }
            USER_LEFT_ROOM => Self::UserLeftRoom { room: r.str()?, username: r.str()? },
            CONNECT_TO_PEER => {
                let username = r.str()?;
                let conn_type = r.str()?;
                let ip = r.ip()?;
                let port = r.u32()?;
                let token = r.u32()?;
                let privileged = if r.is_empty() { false } else { r.bool()? };
                Self::ConnectToPeer { username, conn_type, ip, port, token, privileged }
            }
            MESSAGE_USER => {
                let id = r.u32()?;
                let timestamp = r.u32()?;
                let username = r.str()?;
                let message = r.str()?;
                let is_new = if r.is_empty() { true } else { r.bool()? };
                Self::MessageUser { id, timestamp, username, message, is_new }
            }
            FILE_SEARCH => Self::FileSearch { username: r.str()?, token: r.u32()?, query: r.str()? },
            GET_USER_STATS => Self::UserStats { username: r.str()?, stats: UserStats::read(r)? },
            RELOGGED => Self::Relogged,
            RECOMMENDATIONS => {
                let recommendations = scored(r)?;
                let unrecommendations = scored(r).unwrap_or_default();
                Self::Recommendations { recommendations, unrecommendations }
            }
            GLOBAL_RECOMMENDATIONS => {
                let recommendations = scored(r)?;
                let unrecommendations = scored(r).unwrap_or_default();
                Self::GlobalRecommendations { recommendations, unrecommendations }
            }
            USER_INTERESTS => Self::UserInterests { username: r.str()?, likes: r.strs()?, hates: r.strs()? },
            ROOM_LIST => {
                let names = r.strs()?;
                let counts = r.list(|r| r.u32())?;
                let rooms = names.into_iter().zip(counts).collect();
                let mut private_rooms = Vec::new();
                // owned private rooms, then private rooms we are a member of
                for _ in 0..2 {
                    if r.is_empty() {
                        break;
                    }
                    let n = r.strs()?;
                    let c = r.list(|r| r.u32())?;
                    private_rooms.extend(n.into_iter().zip(c));
                }
                Self::RoomList { rooms, private_rooms }
            }
            ADMIN_MESSAGE => Self::AdminMessage { message: r.str()? },
            PRIVILEGED_USERS => Self::PrivilegedUsers { users: r.strs()? },
            PARENT_MIN_SPEED => Self::ParentMinSpeed { speed: r.u32()? },
            PARENT_SPEED_RATIO => Self::ParentSpeedRatio { ratio: r.u32()? },
            CHECK_PRIVILEGES => Self::CheckPrivileges { seconds_left: r.u32()? },
            EMBEDDED_MESSAGE => Self::EmbeddedMessage { distrib_code: r.u8()?, payload: r.rest().to_vec() },
            POSSIBLE_PARENTS => Self::PossibleParents { parents: r.list(|r| Ok((r.str()?, r.ip()?, r.u32()?)))? },
            WISHLIST_INTERVAL => Self::WishlistInterval { seconds: r.u32()? },
            SIMILAR_USERS => Self::SimilarUsers { users: r.list(|r| Ok((r.str()?, r.u32()?)))? },
            ITEM_RECOMMENDATIONS => Self::ItemRecommendations { item: r.str()?, recommendations: scored(r)? },
            ITEM_SIMILAR_USERS => Self::ItemSimilarUsers { item: r.str()?, users: r.strs()? },
            ROOM_TICKER_STATE => {
                Self::RoomTickerState { room: r.str()?, tickers: r.list(|r| Ok((r.str()?, r.str()?)))? }
            }
            ROOM_TICKER_ADD => Self::RoomTickerAdd { room: r.str()?, username: r.str()?, ticker: r.str()? },
            ROOM_TICKER_REMOVE => Self::RoomTickerRemove { room: r.str()?, username: r.str()? },
            RESET_DISTRIBUTED => Self::ResetDistributed,
            EXCLUDED_SEARCH_PHRASES => Self::ExcludedSearchPhrases { phrases: r.strs()? },
            CANT_CONNECT_TO_PEER => Self::CantConnectToPeer { token: r.u32()? },
            CANT_CREATE_ROOM => Self::CantCreateRoom { room: r.str()? },
            code => Self::Unknown { code },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_encoding() {
        let f = ServerRequest::Login { username: "u".into(), password: "p".into() }.encode();
        let mut r = Reader::new(&f);
        assert_eq!(r.u32().unwrap() as usize, f.len() - 4);
        assert_eq!(r.u32().unwrap(), code::LOGIN);
        assert_eq!(r.str().unwrap(), "u");
        assert_eq!(r.str().unwrap(), "p");
        assert_eq!(r.u32().unwrap(), CLIENT_VERSION);
        assert_eq!(r.str().unwrap(), crate::login_hash("u", "p"));
    }

    #[test]
    fn decode_connect_to_peer() {
        let mut w = Writer::new();
        w.u32(code::CONNECT_TO_PEER).str("bob").str("P").ip(Ipv4Addr::new(10, 0, 0, 1)).u32(2234).u32(77).bool(false);
        let m = ServerResponse::decode(&w.buf).unwrap();
        assert_eq!(
            m,
            ServerResponse::ConnectToPeer {
                username: "bob".into(),
                conn_type: "P".into(),
                ip: Ipv4Addr::new(10, 0, 0, 1),
                port: 2234,
                token: 77,
                privileged: false
            }
        );
    }

    #[test]
    fn decode_join_room() {
        let mut w = Writer::new();
        w.u32(code::JOIN_ROOM).str("room").strs(&["a", "b"]);
        w.u32(2).u32(2).u32(1);
        w.u32(2);
        UserStats { avg_speed: 1, upload_count: 2, files: 3, dirs: 4 }.write(&mut w);
        UserStats::default().write(&mut w);
        w.u32(2).u32(0).u32(1);
        w.strs(&["US", "DE"]);
        match ServerResponse::decode(&w.buf).unwrap() {
            ServerResponse::JoinRoom { room, users, .. } => {
                assert_eq!(room, "room");
                assert_eq!(users.len(), 2);
                assert_eq!(users[0].stats.files, 3);
                assert_eq!(users[1].status, 1);
                assert_eq!(users[1].country.as_deref(), Some("DE"));
            }
            m => panic!("unexpected {m:?}"),
        }
    }
}
