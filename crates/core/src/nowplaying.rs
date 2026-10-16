//! Now playing: set by API or media-server webhooks, shown in our user description.

use crate::client::Client;
use crate::model::*;
use anyhow::{bail, Result};
use serde_json::Value;

const KEY: &str = "nowPlaying";

fn str_at<'a>(v: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut cur = v;
    for p in path {
        cur = cur.get(p)?;
    }
    cur.as_str().map(str::trim).filter(|s| !s.is_empty())
}

impl Client {
    pub fn now_playing(&self) -> Option<NowPlaying> {
        self.now_playing.lock().clone()
    }

    pub fn set_now_playing(&self, artist: &str, title: &str, album: Option<&str>, source: &str) -> Result<NowPlaying> {
        let (artist, title) = (artist.trim(), title.trim());
        if artist.is_empty() && title.is_empty() {
            bail!("artist or title is required");
        }
        let np = NowPlaying {
            artist: artist.to_owned(),
            title: title.to_owned(),
            album: album.map(str::trim).filter(|a| !a.is_empty()).map(str::to_owned),
            source: source.to_owned(),
            started_at: chrono::Utc::now(),
        };
        self.db.kv_set(KEY, &serde_json::to_string(&np).unwrap_or_default());
        *self.now_playing.lock() = Some(np.clone());
        self.emit_app();
        Ok(np)
    }

    pub fn clear_now_playing(&self) {
        if self.now_playing.lock().take().is_some() {
            self.db.kv_set(KEY, "null");
            self.emit_app();
        }
    }

    /// Jellyfin webhook plugin (JSON template with NotificationType/Artist/Name/Album/ItemType).
    pub fn jellyfin_webhook(&self, v: &Value) -> Result<()> {
        let kind = str_at(v, &["NotificationType"]).unwrap_or_default();
        if let Some(t) = str_at(v, &["ItemType"]) {
            if !t.eq_ignore_ascii_case("audio") {
                return Ok(());
            }
        }
        match kind {
            "PlaybackStart" | "PlaybackProgress" => {
                let artist = str_at(v, &["Artist"]).or_else(|| str_at(v, &["AlbumArtist"])).unwrap_or_default();
                let title = str_at(v, &["Name"]).unwrap_or_default();
                let album = str_at(v, &["Album"]);
                if kind == "PlaybackProgress" {
                    if let Some(cur) = self.now_playing() {
                        if cur.artist == artist && cur.title == title {
                            return Ok(());
                        }
                    }
                }
                self.set_now_playing(artist, title, album, "jellyfin")?;
            }
            "PlaybackStop" => self.clear_now_playing(),
            _ => {}
        }
        Ok(())
    }

    /// Plex webhook (`payload` JSON from the multipart body).
    pub fn plex_webhook(&self, v: &Value) -> Result<()> {
        if let Some(t) = str_at(v, &["Metadata", "type"]) {
            if t != "track" {
                return Ok(());
            }
        }
        match str_at(v, &["event"]).unwrap_or_default() {
            "media.play" | "media.resume" => {
                let artist = str_at(v, &["Metadata", "originalTitle"])
                    .or_else(|| str_at(v, &["Metadata", "grandparentTitle"]))
                    .unwrap_or_default();
                let title = str_at(v, &["Metadata", "title"]).unwrap_or_default();
                let album = str_at(v, &["Metadata", "parentTitle"]);
                self.set_now_playing(artist, title, album, "plex")?;
            }
            "media.stop" | "media.pause" => self.clear_now_playing(),
            _ => {}
        }
        Ok(())
    }
}
