//! Serializable state shared with API consumers (see docs/API.md).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use slsk_proto::{attr, FileEntry};

pub type Time = DateTime<Utc>;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum ServerState {
    #[default]
    Disconnected,
    Connecting,
    LoggedIn,
    LoginFailed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum UserStatus {
    #[default]
    Offline,
    Away,
    Online,
}

impl UserStatus {
    pub fn from_code(c: u32) -> Self {
        match c {
            1 => Self::Away,
            2 => Self::Online,
            _ => Self::Offline,
        }
    }
    pub fn code(self) -> u32 {
        match self {
            Self::Offline => 0,
            Self::Away => 1,
            Self::Online => 2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct SearchFile {
    pub filename: String,
    pub size: u64,
    pub extension: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bit_rate: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bit_depth: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub length: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_variable_bit_rate: Option<bool>,
}

impl From<&FileEntry> for SearchFile {
    fn from(f: &FileEntry) -> Self {
        let mut extension = f.extension.to_lowercase();
        if extension.is_empty() {
            extension = f.filename.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
        }
        Self {
            filename: f.filename.clone(),
            size: f.size,
            extension,
            bit_rate: f.attr(attr::BITRATE),
            bit_depth: f.attr(attr::BIT_DEPTH),
            sample_rate: f.attr(attr::SAMPLE_RATE),
            length: f.attr(attr::DURATION),
            is_variable_bit_rate: f.attr(attr::VBR).map(|v| v != 0),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SearchState {
    InProgress,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Search {
    pub id: String,
    pub query: String,
    pub state: SearchState,
    pub started_at: Time,
    pub ended_at: Option<Time>,
    pub response_count: usize,
    pub file_count: usize,
    pub locked_file_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    pub username: String,
    pub has_free_upload_slot: bool,
    pub upload_speed: u32,
    pub queue_length: u32,
    pub score: f64,
    pub reputation: i32,
    pub files: Vec<SearchFile>,
    pub locked_files: Vec<SearchFile>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    Download,
    Upload,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TransferState {
    Queued,
    RemoteQueued,
    Requested,
    InProgress,
    Succeeded,
    Failed,
    Cancelled,
    Rejected,
}

impl TransferState {
    pub fn is_finished(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled | Self::Rejected)
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::RemoteQueued => "remoteQueued",
            Self::Requested => "requested",
            Self::InProgress => "inProgress",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Rejected => "rejected",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SourceState {
    Idle,
    Queued,
    Active,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferSource {
    pub username: String,
    pub filename: String,
    pub state: SourceState,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transfer {
    pub id: String,
    pub direction: Direction,
    pub username: String,
    pub filename: String,
    pub size: u64,
    pub state: TransferState,
    pub state_detail: Option<String>,
    pub bytes_transferred: u64,
    pub average_speed: f64,
    pub place_in_queue: Option<u32>,
    pub requested_at: Time,
    pub started_at: Option<Time>,
    pub ended_at: Option<Time>,
    pub local_path: Option<String>,
    pub sources: Vec<TransferSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInfo {
    pub description: String,
    pub has_picture: bool,
    pub upload_slots: u32,
    pub queue_length: u32,
    pub has_free_upload_slot: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserStatusInfo {
    pub status: UserStatus,
    pub is_privileged: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UserStats {
    pub average_speed: u32,
    pub upload_count: u32,
    pub file_count: u32,
    pub directory_count: u32,
}

impl From<&slsk_proto::server::UserStats> for UserStats {
    fn from(s: &slsk_proto::server::UserStats) -> Self {
        Self { average_speed: s.avg_speed, upload_count: s.upload_count, file_count: s.files, directory_count: s.dirs }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Directory {
    pub name: String,
    pub files: Vec<SearchFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseResult {
    pub directories: Vec<Directory>,
    pub locked_directories: Vec<Directory>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Reputation {
    pub score: i32,
    pub downloads_succeeded: u64,
    pub downloads_failed: u64,
    pub uploads_succeeded: u64,
    pub bytes_downloaded: u64,
    pub bytes_uploaded: u64,
    pub violations: u64,
}

impl Reputation {
    pub fn compute_score(&mut self) {
        let good = self.downloads_succeeded as f64 + self.uploads_succeeded as f64 * 0.5;
        let bad = self.downloads_failed as f64;
        let base = 100.0 * (good - bad) / (good + bad + 4.0);
        self.score = (base - self.violations as f64 * 15.0).round().clamp(-100.0, 100.0) as i32;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserProfile {
    pub username: String,
    pub note: String,
    pub group: Option<String>,
    pub reputation: Reputation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub timestamp: Time,
    pub username: String,
    pub direction: MessageDirection,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MessageDirection {
    In,
    Out,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub username: String,
    pub unread: u32,
    pub last_message: Option<Message>,
    pub status: UserStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomUser {
    pub username: String,
    pub status: UserStatus,
    pub average_speed: u32,
    pub file_count: u32,
    pub country: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomMessage {
    pub timestamp: Time,
    pub username: String,
    pub message: String,
    #[serde(rename = "self")]
    pub is_self: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomSummary {
    pub name: String,
    pub user_count: u32,
    pub is_private: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ticker {
    pub username: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Room {
    pub name: String,
    pub is_private: bool,
    pub users: Vec<RoomUser>,
    pub messages: Vec<RoomMessage>,
    pub tickers: Vec<Ticker>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ShareDirSummary {
    /// Host path; kept server-side only.
    #[serde(skip_serializing, default)]
    pub path: String,
    pub alias: String,
    pub file_count: usize,
    pub directory_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ShareSummary {
    pub directories: Vec<ShareDirSummary>,
    pub file_count: usize,
    pub directory_count: usize,
    pub scanning: bool,
    pub scan_progress: f64,
    pub last_scan: Option<Time>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WishlistFilter {
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub min_bit_rate: Option<u32>,
    #[serde(default)]
    pub min_size: Option<u64>,
    #[serde(default)]
    pub max_size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WishlistItem {
    pub id: String,
    pub query: String,
    pub enabled: bool,
    pub auto_download: bool,
    pub filter: WishlistFilter,
    pub created_at: Time,
    pub last_run_at: Option<Time>,
    pub last_match_count: usize,
    pub last_search_id: Option<String>,
    pub downloaded_count: usize,
    /// Origin of the item, e.g. `lidarr:123`. Not part of the public contract.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NowPlaying {
    pub artist: String,
    pub title: String,
    pub album: Option<String>,
    pub source: String,
    pub started_at: Time,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub timestamp: Time,
    pub level: String,
    pub target: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Violation {
    pub username: String,
    pub kind: String,
    pub count: u64,
    pub last_at: Time,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ban {
    pub username: String,
    pub reason: String,
    pub until: Option<Time>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuardState {
    pub violations: Vec<Violation>,
    pub bans: Vec<Ban>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Metrics {
    pub uptime_seconds: u64,
    pub bytes_downloaded: u64,
    pub bytes_uploaded: u64,
    pub downloads_succeeded: u64,
    pub downloads_failed: u64,
    pub uploads_succeeded: u64,
    pub uploads_failed: u64,
    pub searches_sent: u64,
    pub search_requests_received: u64,
    pub search_responses_sent: u64,
    pub peer_connections: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    pub state: ServerState,
    pub username: Option<String>,
    pub address: String,
    pub error: Option<String>,
    pub is_privileged: bool,
    pub privilege_seconds_left: u32,
    pub status: UserStatus,
    pub listen_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DistributedInfo {
    pub has_parent: bool,
    pub parent: Option<String>,
    pub branch_level: i32,
    pub branch_root: Option<String>,
    pub searches_received: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferSummary {
    pub download_speed: f64,
    pub upload_speed: f64,
    pub active_downloads: usize,
    pub active_uploads: usize,
    pub queued_uploads: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    pub version: String,
    pub server: ServerInfo,
    pub shares: ShareSummary,
    pub distributed: DistributedInfo,
    pub transfers: TransferSummary,
    pub now_playing: Option<NowPlaying>,
    pub unread_messages: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoredItem {
    pub item: String,
    pub score: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recommendations {
    pub recommendations: Vec<ScoredItem>,
    pub unrecommendations: Vec<ScoredItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Interests {
    pub likes: Vec<String>,
    pub hates: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimilarUser {
    pub username: String,
    pub rating: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemInfo {
    pub recommendations: Vec<ScoredItem>,
    pub similar_users: Vec<String>,
}

/// Events pushed to API clients.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum Event {
    #[serde(rename = "app")]
    App(Box<AppState>),
    #[serde(rename = "search.updated")]
    SearchUpdated(Search),
    #[serde(rename = "search.removed")]
    SearchRemoved { id: String },
    #[serde(rename = "transfer.updated")]
    TransferUpdated(Box<Transfer>),
    #[serde(rename = "transfer.removed")]
    TransferRemoved { id: String },
    #[serde(rename = "message")]
    Message(Message),
    #[serde(rename = "room.message")]
    RoomMessage { room: String, message: RoomMessage },
    #[serde(rename = "room.updated")]
    RoomUpdated(Box<Room>),
    #[serde(rename = "room.left")]
    RoomLeft { name: String },
    #[serde(rename = "wishlist.updated")]
    WishlistUpdated(WishlistItem),
    #[serde(rename = "log")]
    Log(LogEntry),
    /// A user's country (ISO 3166-1 alpha-2) became known or changed.
    #[serde(rename = "user.country")]
    UserCountry { username: String, country: String },
}

/// Split a Soulseek path into (directory, basename).
pub fn split_path(path: &str) -> (&str, &str) {
    match path.rfind(['\\', '/']) {
        Some(i) => (&path[..i], &path[i + 1..]),
        None => ("", path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_shape() {
        let e = Event::SearchRemoved { id: "x".into() };
        assert_eq!(serde_json::to_string(&e).unwrap(), r#"{"type":"search.removed","data":{"id":"x"}}"#);
        let e = Event::UserCountry { username: "u".into(), country: "DE".into() };
        assert_eq!(
            serde_json::to_string(&e).unwrap(),
            r#"{"type":"user.country","data":{"username":"u","country":"DE"}}"#
        );
    }

    #[test]
    fn reputation_score() {
        let mut r = Reputation { downloads_succeeded: 10, ..Default::default() };
        r.compute_score();
        assert!(r.score > 50);
        r.violations = 20;
        r.compute_score();
        assert_eq!(r.score, -100);
    }
}
