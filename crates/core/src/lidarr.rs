//! Lidarr: import wanted (missing) albums into the wishlist and ask Lidarr to
//! scan completed downloads.

use crate::client::Client;
use crate::model::*;
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LidarrStatus {
    pub enabled: bool,
    pub last_sync: Option<Time>,
    pub last_error: Option<String>,
    pub last_added: usize,
    /// Folders waiting for a debounced import scan.
    #[serde(skip)]
    pending_scans: HashSet<String>,
}

const IMPORT_DEBOUNCE: Duration = Duration::from_secs(30);

impl Client {
    fn lidarr_http(&self) -> Result<(reqwest::Client, String, String)> {
        let c = self.config.read().integrations.lidarr.clone();
        if !c.enabled {
            bail!("Lidarr integration is disabled");
        }
        if c.url.trim().is_empty() || c.api_key.trim().is_empty() {
            bail!("Lidarr URL and API key are required");
        }
        let http = reqwest::Client::builder().timeout(Duration::from_secs(30)).build()?;
        Ok((http, c.url.trim_end_matches('/').to_owned(), c.api_key))
    }

    pub fn lidarr_status(&self) -> LidarrStatus {
        let mut s = self.lidarr.lock().clone();
        s.enabled = self.config.read().integrations.lidarr.enabled;
        s
    }

    /// Add Lidarr's missing albums to the wishlist. Returns how many were added.
    pub async fn lidarr_sync(&self) -> Result<usize> {
        let r = self.lidarr_sync_inner().await;
        let mut s = self.lidarr.lock();
        s.last_sync = Some(chrono::Utc::now());
        match &r {
            Ok(n) => {
                s.last_added = *n;
                s.last_error = None;
            }
            Err(e) => s.last_error = Some(format!("{e:#}")),
        }
        r
    }

    async fn lidarr_sync_inner(&self) -> Result<usize> {
        let (http, base, key) = self.lidarr_http()?;
        let v: serde_json::Value = http
            .get(format!("{base}/api/v1/wanted/missing"))
            .query(&[("page", "1"), ("pageSize", "500"), ("includeArtist", "true"), ("monitored", "true")])
            .header("X-Api-Key", key)
            .send()
            .await
            .context("contacting Lidarr")?
            .error_for_status()
            .context("Lidarr wanted/missing")?
            .json()
            .await
            .context("decoding Lidarr response")?;
        let existing: HashSet<String> = self.wishlist().into_iter().filter_map(|w| w.external_id).collect();
        let mut added = 0;
        for rec in v.get("records").and_then(|r| r.as_array()).into_iter().flatten() {
            let Some(id) = rec.get("id").and_then(|i| i.as_i64()) else { continue };
            let ext = format!("lidarr:{id}");
            if existing.contains(&ext) {
                continue;
            }
            let title = rec.get("title").and_then(|t| t.as_str()).unwrap_or_default();
            let artist = rec
                .get("artist")
                .and_then(|a| a.get("artistName"))
                .and_then(|a| a.as_str())
                .unwrap_or_default();
            let query = format!("{artist} {title}");
            if query.trim().is_empty() {
                continue;
            }
            self.add_wishlist(&query, true, WishlistFilter::default(), Some(ext))?;
            added += 1;
        }
        tracing::info!("lidarr sync: {added} albums added to wishlist");
        Ok(added)
    }

    pub(crate) async fn lidarr_loop(self: Arc<Self>) {
        tokio::time::sleep(Duration::from_secs(90)).await;
        loop {
            let (enabled, minutes) = {
                let c = self.config.read();
                (c.integrations.lidarr.enabled, c.integrations.lidarr.sync_interval_minutes)
            };
            if enabled && minutes > 0 {
                let due = self
                    .lidarr
                    .lock()
                    .last_sync
                    .is_none_or(|t| chrono::Utc::now() - t >= chrono::Duration::minutes(minutes as i64));
                if due {
                    if let Err(e) = self.lidarr_sync().await {
                        tracing::warn!("lidarr sync: {e:#}");
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    }

    /// Called when a download finishes; asks Lidarr to import the folder if the
    /// download came from a Lidarr wishlist item.
    pub(crate) fn on_download_complete(self: &Arc<Self>, t: &Transfer) {
        let ext: Option<String> = self.db.get("lidarr_transfer", &t.id);
        if ext.is_none() {
            return;
        }
        self.db.delete("lidarr_transfer", &t.id);
        let c = self.config.read().integrations.lidarr.clone();
        if !c.enabled || !c.import_on_complete {
            return;
        }
        let Some(folder) = t
            .local_path
            .as_deref()
            .and_then(|p| std::path::Path::new(p).parent())
            .map(|p| p.to_string_lossy().into_owned())
        else {
            return;
        };
        if !self.lidarr.lock().pending_scans.insert(folder.clone()) {
            return;
        }
        let me = self.clone();
        tokio::spawn(async move {
            // wait for the rest of the album
            tokio::time::sleep(IMPORT_DEBOUNCE).await;
            me.lidarr.lock().pending_scans.remove(&folder);
            if let Err(e) = me.lidarr_import(&folder).await {
                tracing::warn!("lidarr import of {folder}: {e:#}");
                me.lidarr.lock().last_error = Some(format!("{e:#}"));
            }
        });
    }

    async fn lidarr_import(&self, folder: &str) -> Result<()> {
        let (http, base, key) = self.lidarr_http()?;
        http.post(format!("{base}/api/v1/command"))
            .header("X-Api-Key", key)
            .json(&serde_json::json!({ "name": "DownloadedAlbumsScan", "path": folder }))
            .send()
            .await
            .context("contacting Lidarr")?
            .error_for_status()?;
        tracing::info!("lidarr: requested import of {folder}");
        Ok(())
    }
}
