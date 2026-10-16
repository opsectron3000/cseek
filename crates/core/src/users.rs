//! Users: remote lookups (info, status, stats, browse), local profiles (notes,
//! groups, reputation) and group resolution for upload policy.

use crate::client::Client;
use crate::config::GroupConfig;
use crate::model::*;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use slsk_proto::{PeerMessage, ServerRequest, ServerResponse};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

pub const GROUP_BLACKLISTED: &str = "blacklisted";
pub const GROUP_LEECHERS: &str = "leechers";
const GROUP_DEFAULT: &str = "default";
const GROUP_PRIVILEGED: &str = "privileged";

/// Effective upload policy for a user.
#[derive(Debug, Clone)]
pub(crate) struct GroupInfo {
    pub name: String,
    pub priority: u32,
    pub upload_slots: u32,
    pub speed_limit_kbps: u64,
    pub allowed_extensions: Option<HashSet<String>>,
    pub queue_limit_files: u32,
}

#[derive(Serialize, Deserialize)]
struct RepDoc {
    username: String,
    reputation: Reputation,
}

fn ext_set(v: &[String]) -> Option<HashSet<String>> {
    let s: HashSet<String> =
        v.iter().map(|e| e.trim().trim_start_matches('.').to_lowercase()).filter(|e| !e.is_empty()).collect();
    (!s.is_empty()).then_some(s)
}

fn to_directory(d: slsk_proto::DirEntry) -> Directory {
    Directory { name: d.name, files: d.files.iter().map(SearchFile::from).collect() }
}

impl Client {
    pub fn is_blacklisted(&self, username: &str) -> bool {
        self.config.read().blacklist.iter().any(|u| u == username)
    }

    pub(crate) fn user_group_info(&self, username: &str) -> GroupInfo {
        let cfg = self.config.read();
        if let Some(g) = cfg.groups.iter().find(|g| g.members.iter().any(|m| m == username)) {
            return GroupInfo {
                name: g.name.clone(),
                priority: g.priority,
                upload_slots: g.upload_slots,
                speed_limit_kbps: g.speed_limit_kbps,
                allowed_extensions: ext_set(&g.allowed_extensions),
                queue_limit_files: g.queue_limit_files,
            };
        }
        let leech = &cfg.leechers;
        let leecher = self.db.get::<bool>("leecher", username).unwrap_or(false)
            || (leech.enabled
                && !self.privileged_users.lock().contains(username)
                && self
                    .user_stats
                    .lock()
                    .get(username)
                    .is_some_and(|(s, _)| s.file_count < leech.min_files || s.directory_count < leech.min_directories));
        if leecher {
            return GroupInfo {
                name: GROUP_LEECHERS.into(),
                priority: 1000,
                upload_slots: 0,
                speed_limit_kbps: leech.speed_limit_kbps,
                allowed_extensions: ext_set(&leech.allowed_extensions),
                queue_limit_files: 0,
            };
        }
        let privileged = self.privileged_users.lock().contains(username);
        GroupInfo {
            name: if privileged { GROUP_PRIVILEGED } else { GROUP_DEFAULT }.into(),
            priority: if privileged { 0 } else { 100 },
            upload_slots: 0,
            speed_limit_kbps: 0,
            allowed_extensions: None,
            queue_limit_files: 0,
        }
    }

    /// slskdn: file types this user may search for / download. `None` = everything.
    pub(crate) fn allowed_extensions_for(&self, username: &str) -> Option<HashSet<String>> {
        self.user_group_info(username).allowed_extensions
    }

    /// The group explicitly assigned to a user (not automatic leecher detection).
    fn assigned_group(&self, username: &str) -> Option<String> {
        if self.is_blacklisted(username) {
            return Some(GROUP_BLACKLISTED.into());
        }
        let cfg = self.config.read();
        if let Some(g) = cfg.groups.iter().find(|g| g.members.iter().any(|m| m == username)) {
            return Some(g.name.clone());
        }
        if self.db.get::<bool>("leecher", username).unwrap_or(false) {
            return Some(GROUP_LEECHERS.into());
        }
        None
    }

    // ---------- reputation ----------

    pub fn reputation(&self, username: &str) -> Reputation {
        self.db.get::<RepDoc>("reputation", username).map(|d| d.reputation).unwrap_or_default()
    }

    pub(crate) fn record_reputation(&self, username: &str, f: impl FnOnce(&mut Reputation)) {
        if username.is_empty() {
            return;
        }
        let mut r = self.reputation(username);
        f(&mut r);
        r.compute_score();
        self.db.put("reputation", username, &RepDoc { username: username.to_owned(), reputation: r });
    }

    pub fn reputations(&self) -> HashMap<String, i32> {
        self.db.all::<RepDoc>("reputation").into_iter().map(|d| (d.username, d.reputation.score)).collect()
    }

    // ---------- local profile ----------

    pub fn profile(&self, username: &str) -> UserProfile {
        UserProfile {
            username: username.to_owned(),
            note: self.db.get::<String>("note", username).unwrap_or_default(),
            group: self.assigned_group(username),
            reputation: self.reputation(username),
        }
    }

    pub fn set_note(&self, username: &str, note: &str) -> UserProfile {
        if note.trim().is_empty() {
            self.db.delete("note", username);
        } else {
            self.db.put("note", username, &note.to_owned());
        }
        self.profile(username)
    }

    /// Assign a user to a group (`blacklisted`, `leechers`, a custom group) or back to default.
    pub fn set_group(self: &Arc<Self>, username: &str, group: Option<&str>) -> Result<UserProfile> {
        let group = group.map(str::trim).filter(|g| !g.is_empty());
        if let Some(g) = group {
            if g != GROUP_BLACKLISTED && g != GROUP_LEECHERS && !self.config.read().groups.iter().any(|x| x.name == g) {
                bail!("unknown group {g:?}");
            }
        }
        let u = username.to_owned();
        let target = group.map(str::to_owned);
        self.update_config(|c| {
            c.blacklist.retain(|x| x != &u);
            for g in &mut c.groups {
                g.members.retain(|m| m != &u);
            }
            match target.as_deref() {
                Some(GROUP_BLACKLISTED) => c.blacklist.push(u.clone()),
                Some(GROUP_LEECHERS) | None => {}
                Some(name) => {
                    if let Some(g) = c.groups.iter_mut().find(|g| g.name == name) {
                        g.members.push(u.clone());
                    }
                }
            }
        })?;
        if group == Some(GROUP_LEECHERS) {
            self.db.put("leecher", username, &true);
        } else {
            self.db.delete("leecher", username);
        }
        Ok(self.profile(username))
    }

    pub fn custom_groups(&self) -> Vec<GroupConfig> {
        self.config.read().groups.clone()
    }

    // ---------- remote lookups ----------

    pub async fn user_info(self: &Arc<Self>, username: &str) -> Result<UserInfo> {
        let r = self
            .peer_request(username, PeerMessage::UserInfoRequest, format!("info:{username}"), Duration::from_secs(20))
            .await?;
        match r {
            PeerMessage::UserInfoResponse(i) => Ok(UserInfo {
                description: i.description,
                has_picture: i.picture.is_some(),
                upload_slots: i.total_uploads,
                queue_length: i.queue_size,
                has_free_upload_slot: i.slots_free,
            }),
            _ => bail!("unexpected reply"),
        }
    }

    /// Picture from the last user info response, fetching info if needed.
    pub async fn user_picture(self: &Arc<Self>, username: &str) -> Option<Vec<u8>> {
        if let Some(p) = self.pictures.lock().get(username).cloned() {
            return Some(p);
        }
        self.user_info(username).await.ok()?;
        self.pictures.lock().get(username).cloned()
    }

    pub async fn user_status(&self, username: &str) -> Result<UserStatusInfo> {
        let r = self
            .server_request(
                ServerRequest::GetUserStatus { username: username.to_owned() },
                format!("status:{username}"),
                Duration::from_secs(10),
            )
            .await?;
        match r {
            ServerResponse::UserStatus { status, privileged, .. } => {
                Ok(UserStatusInfo { status: UserStatus::from_code(status), is_privileged: privileged })
            }
            _ => bail!("unexpected reply"),
        }
    }

    pub async fn user_stats(&self, username: &str) -> Result<UserStats> {
        let r = self
            .server_request(
                ServerRequest::GetUserStats { username: username.to_owned() },
                format!("stats:{username}"),
                Duration::from_secs(10),
            )
            .await?;
        match r {
            ServerResponse::UserStats { stats, .. } => Ok((&stats).into()),
            _ => bail!("unexpected reply"),
        }
    }

    pub async fn user_interests(&self, username: &str) -> Result<Interests> {
        let r = self
            .server_request(
                ServerRequest::GetUserInterests { username: username.to_owned() },
                format!("interests:{username}"),
                Duration::from_secs(15),
            )
            .await?;
        match r {
            ServerResponse::UserInterests { likes, hates, .. } => Ok(Interests { likes, hates }),
            _ => bail!("unexpected reply"),
        }
    }

    pub async fn browse(self: &Arc<Self>, username: &str) -> Result<BrowseResult> {
        let r = self
            .peer_request(username, PeerMessage::SharedFileListRequest, format!("browse:{username}"), Duration::from_secs(120))
            .await?;
        match r {
            PeerMessage::SharedFileListResponse { dirs, private_dirs } => Ok(BrowseResult {
                directories: dirs.into_iter().map(to_directory).collect(),
                locked_directories: private_dirs.into_iter().map(to_directory).collect(),
            }),
            _ => bail!("unexpected reply"),
        }
    }

    pub async fn directory(self: &Arc<Self>, username: &str, folder: &str) -> Result<Vec<Directory>> {
        let token = self.token();
        let r = self
            .peer_request(
                username,
                PeerMessage::FolderContentsRequest { token, folder: folder.to_owned() },
                format!("folder:{username}:{token}"),
                Duration::from_secs(30),
            )
            .await?;
        match r {
            PeerMessage::FolderContentsResponse { dirs, .. } => Ok(dirs.into_iter().map(to_directory).collect()),
            _ => bail!("unexpected reply"),
        }
    }

    /// Cached status, as last reported by the server.
    pub fn cached_status(&self, username: &str) -> UserStatus {
        self.user_status.lock().get(username).map(|s| s.0).unwrap_or_default()
    }

}
