//! Daemon configuration. Stored as TOML; exposed over the API as camelCase JSON.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const REDACTED: &str = "********";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    pub soulseek: SoulseekConfig,
    pub directories: DirectoriesConfig,
    pub shares: SharesConfig,
    pub transfers: TransfersConfig,
    pub groups: Vec<GroupConfig>,
    pub blacklist: Vec<String>,
    pub leechers: LeechersConfig,
    pub search: SearchConfig,
    pub rooms: RoomsConfig,
    pub wishlist: WishlistConfig,
    pub guard: GuardConfig,
    pub now_playing: NowPlayingConfig,
    pub integrations: IntegrationsConfig,
    pub geoip: GeoipConfig,
    pub web: WebConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SoulseekConfig {
    pub username: String,
    pub password: String,
    pub server_host: String,
    pub server_port: u16,
    pub listen_port: u16,
    pub description: String,
    pub picture: Option<String>,
    pub auto_connect: bool,
}

impl Default for SoulseekConfig {
    fn default() -> Self {
        Self {
            username: String::new(),
            password: String::new(),
            server_host: "server.slsknet.org".into(),
            server_port: 2242,
            listen_port: 50300,
            description: "cseek — a headless Soulseek client".into(),
            picture: None,
            auto_connect: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Destination {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DirectoriesConfig {
    /// The storage folder. Every other path here, and every shared folder, is
    /// relative to it and can never point outside it. Relative to the data
    /// directory unless absolute; only settable in the config file.
    pub root: String,
    pub downloads: String,
    pub incomplete: String,
    /// Extra named download destinations (slskdn: multiple download destinations).
    pub destinations: Vec<Destination>,
}

impl Default for DirectoriesConfig {
    fn default() -> Self {
        Self { root: "files".into(), downloads: "downloads".into(), incomplete: "incomplete".into(), destinations: vec![] }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SharesConfig {
    /// Directories to share. `"[Alias]/path"` sets an explicit alias, otherwise the directory name is used.
    pub directories: Vec<String>,
    /// Case-insensitive substrings; matching paths are not shared.
    pub exclude_filters: Vec<String>,
    pub rescan_interval_minutes: u64,
}

impl Default for SharesConfig {
    fn default() -> Self {
        Self { directories: vec![], exclude_filters: vec![".DS_Store".into(), "Thumbs.db".into()], rescan_interval_minutes: 720 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TransfersConfig {
    pub upload_slots: u32,
    pub upload_speed_limit_kbps: u64,
    pub download_speed_limit_kbps: u64,
    pub max_queued_files_per_user: u32,
    pub max_queued_bytes_per_user: u64,
    /// Download a file from several sources at once, in chunks.
    pub multi_source: bool,
    pub multi_source_chunk_mb: u64,
    pub stalled_timeout_seconds: u64,
    /// When a download stalls or fails, switch to an alternate source (searching for one if needed).
    pub auto_replace_stalled: bool,
}

impl Default for TransfersConfig {
    fn default() -> Self {
        Self {
            upload_slots: 4,
            upload_speed_limit_kbps: 0,
            download_speed_limit_kbps: 0,
            max_queued_files_per_user: 500,
            max_queued_bytes_per_user: 10 * 1024 * 1024 * 1024,
            multi_source: true,
            multi_source_chunk_mb: 8,
            stalled_timeout_seconds: 90,
            auto_replace_stalled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct GroupConfig {
    pub name: String,
    pub members: Vec<String>,
    /// Max concurrent uploads for this group (0 = only bounded by global slots).
    pub upload_slots: u32,
    pub speed_limit_kbps: u64,
    /// Lower number = served first. Default users have priority 100, leechers 1000.
    pub priority: u32,
    /// slskdn: per-group file type restrictions. Empty = everything allowed.
    pub allowed_extensions: Vec<String>,
    pub queue_limit_files: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LeechersConfig {
    pub enabled: bool,
    pub min_files: u32,
    pub min_directories: u32,
    pub speed_limit_kbps: u64,
    pub allowed_extensions: Vec<String>,
}

impl Default for LeechersConfig {
    fn default() -> Self {
        Self { enabled: false, min_files: 1, min_directories: 1, speed_limit_kbps: 50, allowed_extensions: vec![] }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchConfig {
    pub response_limit: usize,
    pub file_limit: usize,
    pub timeout_seconds: u64,
    /// Max files we include when answering someone else's search.
    pub max_results_per_response: usize,
    /// Local list of phrases we never answer searches for (merged with the server's list).
    pub excluded_phrases: Vec<String>,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self { response_limit: 500, file_limit: 20_000, timeout_seconds: 15, max_results_per_response: 500, excluded_phrases: vec![] }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct RoomsConfig {
    pub auto_join: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WishlistConfig {
    pub enabled: bool,
    pub interval_minutes: u64,
    pub max_auto_downloads_per_run: usize,
}

impl Default for WishlistConfig {
    fn default() -> Self {
        Self { enabled: true, interval_minutes: 60, max_auto_downloads_per_run: 3 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GuardConfig {
    pub enabled: bool,
    pub max_queue_requests_per_minute: u32,
    pub max_search_responses_per_minute: u32,
    pub ban_threshold: u32,
    pub ban_minutes: u64,
}

impl Default for GuardConfig {
    fn default() -> Self {
        Self { enabled: true, max_queue_requests_per_minute: 300, max_search_responses_per_minute: 60, ban_threshold: 5, ban_minutes: 60 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NowPlayingConfig {
    pub enabled: bool,
    /// `{description}`, `{artist}`, `{title}`, `{album}` are substituted.
    pub description_template: String,
}

impl Default for NowPlayingConfig {
    fn default() -> Self {
        Self { enabled: true, description_template: "{description}\n\nNow playing: {artist} – {title}".into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct IntegrationsConfig {
    pub lidarr: LidarrConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LidarrConfig {
    pub enabled: bool,
    pub url: String,
    pub api_key: String,
    pub sync_interval_minutes: u64,
    pub import_on_complete: bool,
}

impl Default for LidarrConfig {
    fn default() -> Self {
        Self { enabled: false, url: "http://localhost:8686".into(), api_key: String::new(), sync_interval_minutes: 360, import_on_complete: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GeoipConfig {
    /// Download DB-IP Country Lite to `<data>/geoip` and look up peer countries locally.
    pub enabled: bool,
}

impl Default for GeoipConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WebConfig {
    pub port: u16,
    pub host: String,
    pub auth: WebAuthConfig,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self { port: 5030, host: "0.0.0.0".into(), auth: WebAuthConfig::default() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WebAuthConfig {
    pub disabled: bool,
    pub username: String,
    pub password: String,
    pub api_keys: Vec<String>,
}

impl Default for WebAuthConfig {
    fn default() -> Self {
        Self { disabled: false, username: "cseek".into(), password: "cseek".into(), api_keys: vec![] }
    }
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&text)?)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, toml::to_string_pretty(self)?)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    /// A copy safe to show in the UI.
    pub fn redacted(&self) -> Self {
        let mut c = self.clone();
        let hide = |s: &mut String| {
            if !s.is_empty() {
                *s = REDACTED.into();
            }
        };
        hide(&mut c.soulseek.password);
        hide(&mut c.web.auth.password);
        hide(&mut c.integrations.lidarr.api_key);
        c.web.auth.api_keys.iter_mut().for_each(hide);
        c
    }

    /// Merge an edited (possibly redacted) config onto this one, keeping secrets that came back redacted.
    pub fn merge_redacted(&self, mut incoming: Config) -> Config {
        let keep = |new: &mut String, old: &str| {
            if new == REDACTED {
                *new = old.to_owned();
            }
        };
        // The storage folder is fixed by whoever runs the daemon.
        incoming.directories.root = self.directories.root.clone();
        keep(&mut incoming.soulseek.password, &self.soulseek.password);
        keep(&mut incoming.web.auth.password, &self.web.auth.password);
        keep(&mut incoming.integrations.lidarr.api_key, &self.integrations.lidarr.api_key);
        for (i, k) in incoming.web.auth.api_keys.iter_mut().enumerate() {
            if k == REDACTED {
                *k = self.web.auth.api_keys.get(i).cloned().unwrap_or_default();
            }
        }
        incoming.web.auth.api_keys.retain(|k| !k.is_empty());
        incoming
    }

    /// The storage folder that confines downloads and shares.
    pub fn storage_root(&self, base: &Path) -> PathBuf {
        let r = self.directories.root.trim();
        let p = expand_home(if r.is_empty() { "files" } else { r });
        if p.is_absolute() {
            p
        } else {
            base.join(p)
        }
    }

    /// `p` inside the storage folder. Absolute paths, `~` and `..` cannot
    /// leave it: a path already under the root is made relative, anything
    /// else is read as relative to the root.
    pub fn resolve(&self, base: &Path, p: &str) -> PathBuf {
        let root = self.storage_root(base);
        root.join(storage_relative(&root, p))
    }

    /// Parse `"[Alias]/path"` share specs into (alias, path).
    pub fn share_roots(&self, base: &Path) -> Vec<(String, PathBuf)> {
        self.shares
            .directories
            .iter()
            .filter(|s| !s.trim().is_empty())
            .map(|spec| {
                let spec = spec.trim();
                if let Some(rest) = spec.strip_prefix('[') {
                    if let Some((alias, path)) = rest.split_once(']') {
                        return (alias.to_owned(), self.resolve(base, path.trim()));
                    }
                }
                let path = self.resolve(base, spec);
                let alias = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "share".into());
                (alias, path)
            })
            .collect()
    }
}

/// `p` as a clean path relative to `root` (lexically; symlinks are the
/// caller's concern). Never contains `..` or a root component.
pub fn storage_relative(root: &Path, p: &str) -> PathBuf {
    use std::path::Component;
    let p = p.trim();
    let raw = Path::new(p);
    let raw = raw.strip_prefix(root).unwrap_or(raw);
    let raw = raw.strip_prefix("~").unwrap_or(raw);
    let mut out = PathBuf::new();
    for c in raw.components() {
        match c {
            Component::Normal(n) => out.push(n),
            Component::ParentDir => {
                out.pop();
            }
            _ => {}
        }
    }
    out
}

/// Whether `p` is a plain path relative to the storage folder.
pub fn is_storage_relative(p: &str) -> bool {
    use std::path::Component;
    let p = p.trim();
    !p.starts_with('~') && Path::new(p).components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

pub fn expand_home(p: &str) -> PathBuf {
    if p == "~" {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home);
        }
    }
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_stay_in_storage() {
        let c = Config::default();
        let base = Path::new("/data");
        let root = Path::new("/data/files");
        assert_eq!(c.storage_root(base), root);
        assert_eq!(c.resolve(base, "downloads"), root.join("downloads"));
        assert_eq!(c.resolve(base, "/etc/passwd"), root.join("etc/passwd"));
        assert_eq!(c.resolve(base, "../../etc"), root.join("etc"));
        assert_eq!(c.resolve(base, "a/../../b"), root.join("b"));
        assert_eq!(c.resolve(base, "~/Music"), root.join("Music"));
        assert_eq!(c.resolve(base, "/data/files/music"), root.join("music"));
        assert_eq!(c.resolve(base, ""), root);
        assert!(is_storage_relative("music/flac"));
        assert!(!is_storage_relative("/music"));
        assert!(!is_storage_relative("../music"));
        assert!(!is_storage_relative("~/music"));
    }

    #[test]
    fn toml_roundtrip_and_redaction() {
        let mut c = Config::default();
        c.soulseek.password = "secret".into();
        c.shares.directories = vec!["[Music]/srv/music".into(), "/srv/books".into()];
        let text = toml::to_string_pretty(&c).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back.soulseek.password, "secret");
        let red = back.redacted();
        assert_eq!(red.soulseek.password, REDACTED);
        let merged = back.merge_redacted(red);
        assert_eq!(merged.soulseek.password, "secret");
        let roots = back.share_roots(Path::new("/"));
        assert_eq!(roots[0], ("Music".into(), PathBuf::from("/files/srv/music")));
        assert_eq!(roots[1].0, "books");
    }

    #[test]
    fn partial_toml_uses_defaults() {
        let c: Config = toml::from_str("[soulseek]\nusername = \"x\"\n").unwrap();
        assert_eq!(c.soulseek.username, "x");
        assert_eq!(c.soulseek.server_port, 2242);
        assert_eq!(c.web.port, 5030);
        assert!(c.geoip.enabled);
    }
}
