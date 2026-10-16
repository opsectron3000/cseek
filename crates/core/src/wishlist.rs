//! Wishlist: saved searches that run in the background and can auto-download
//! the best matching folder.

use crate::client::Client;
use crate::downloads::{DownloadItem, SourceRef};
use crate::model::*;
use anyhow::{anyhow, bail, Result};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

const LOSSLESS: &[&str] = &["flac", "wav", "aiff", "aif", "ape", "wv", "alac"];

/// Partial update accepted by `PUT /wishlist/:id`.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WishlistPatch {
    pub query: Option<String>,
    pub enabled: Option<bool>,
    pub auto_download: Option<bool>,
    pub filter: Option<WishlistFilter>,
}

pub(crate) fn file_matches(f: &SearchFile, filter: &WishlistFilter) -> bool {
    if !filter.extensions.is_empty()
        && !filter.extensions.iter().any(|e| e.trim_start_matches('.').eq_ignore_ascii_case(&f.extension))
    {
        return false;
    }
    if let Some(min) = filter.min_bit_rate {
        // lossless files usually report no bitrate; they always pass
        if !LOSSLESS.contains(&f.extension.as_str()) && f.bit_rate.is_some_and(|b| b < min) {
            return false;
        }
    }
    if filter.min_size.is_some_and(|m| f.size < m) || filter.max_size.is_some_and(|m| f.size > m) {
        return false;
    }
    true
}

/// Pick the best (user, folder) group and build download items with alternate sources.
pub(crate) fn pick_downloads(responses: &[SearchResponse], filter: &WishlistFilter) -> Vec<DownloadItem> {
    let mut best: Option<(f64, &SearchResponse, &str, Vec<&SearchFile>)> = None;
    for r in responses {
        let mut groups: HashMap<&str, Vec<&SearchFile>> = HashMap::new();
        for f in r.files.iter().filter(|f| file_matches(f, filter)) {
            groups.entry(split_path(&f.filename).0).or_default().push(f);
        }
        for (dir, files) in groups {
            // prefer complete folders from good peers
            let s = r.score + (files.len() as f64).ln_1p() * 2.0;
            if best.as_ref().is_none_or(|b| s > b.0) {
                best = Some((s, r, dir, files));
            }
        }
    }
    let Some((_, r, _, files)) = best else { return vec![] };
    files
        .into_iter()
        .map(|f| {
            let base = split_path(&f.filename).1;
            let mut alts: Vec<(&SearchResponse, &SearchFile)> = responses
                .iter()
                .filter(|o| o.username != r.username)
                .filter_map(|o| o.files.iter().find(|g| g.size == f.size && split_path(&g.filename).1 == base).map(|g| (o, g)))
                .collect();
            alts.sort_by(|a, b| b.0.score.total_cmp(&a.0.score));
            DownloadItem {
                username: r.username.clone(),
                filename: f.filename.clone(),
                size: f.size,
                sources: alts
                    .into_iter()
                    .take(4)
                    .map(|(o, g)| SourceRef { username: o.username.clone(), filename: g.filename.clone() })
                    .collect(),
            }
        })
        .collect()
}

impl Client {
    pub fn wishlist(&self) -> Vec<WishlistItem> {
        let mut v: Vec<WishlistItem> = self.db.all("wishlist");
        v.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        v
    }

    pub fn wishlist_item(&self, id: &str) -> Option<WishlistItem> {
        self.db.get("wishlist", id)
    }

    fn save_wishlist_item(&self, w: &WishlistItem) {
        self.db.put("wishlist", &w.id, w);
        self.emit(Event::WishlistUpdated(w.clone()));
    }

    pub fn add_wishlist(&self, query: &str, auto_download: bool, filter: WishlistFilter, external_id: Option<String>) -> Result<WishlistItem> {
        let query = query.trim();
        if query.is_empty() {
            bail!("query is empty");
        }
        let w = WishlistItem {
            id: uuid::Uuid::new_v4().to_string(),
            query: query.to_owned(),
            enabled: true,
            auto_download,
            filter,
            created_at: chrono::Utc::now(),
            last_run_at: None,
            last_match_count: 0,
            last_search_id: None,
            downloaded_count: 0,
            external_id,
        };
        self.save_wishlist_item(&w);
        Ok(w)
    }

    pub fn update_wishlist(&self, id: &str, p: WishlistPatch) -> Result<WishlistItem> {
        let mut w = self.wishlist_item(id).ok_or_else(|| anyhow!("no such wishlist item"))?;
        if let Some(q) = p.query {
            let q = q.trim();
            if q.is_empty() {
                bail!("query is empty");
            }
            w.query = q.to_owned();
        }
        if let Some(e) = p.enabled {
            w.enabled = e;
        }
        if let Some(a) = p.auto_download {
            w.auto_download = a;
        }
        if let Some(f) = p.filter {
            w.filter = f;
        }
        self.save_wishlist_item(&w);
        Ok(w)
    }

    pub fn delete_wishlist(&self, id: &str) -> bool {
        let existed = self.wishlist_item(id).is_some();
        self.db.delete("wishlist", id);
        existed
    }

    /// Start the search for one item and process results in the background.
    pub fn run_wishlist_item(self: &Arc<Self>, id: &str) -> Result<WishlistItem> {
        let w = self.wishlist_item(id).ok_or_else(|| anyhow!("no such wishlist item"))?;
        if !self.is_logged_in() {
            bail!("not connected to the server");
        }
        let search = self.start_search(&w.query)?;
        let w = self.update_wishlist_run(id, &search.id)?;
        let me = self.clone();
        let id = id.to_owned();
        tokio::spawn(async move {
            let _guard = me.wishlist_lock.lock().await;
            if let Err(e) = me.finish_wishlist_run(&id, &search.id, true).await {
                tracing::warn!("wishlist {id}: {e:#}");
            }
        });
        Ok(w)
    }

    fn update_wishlist_run(&self, id: &str, search_id: &str) -> Result<WishlistItem> {
        let mut w = self.wishlist_item(id).ok_or_else(|| anyhow!("no such wishlist item"))?;
        if let Some(old) = w.last_search_id.replace(search_id.to_owned()) {
            if old != search_id {
                self.delete_search(&old);
            }
        }
        w.last_run_at = Some(chrono::Utc::now());
        self.save_wishlist_item(&w);
        Ok(w)
    }

    /// Wait for the search, record matches and auto-download. Returns whether anything was queued.
    async fn finish_wishlist_run(self: &Arc<Self>, id: &str, search_id: &str, allow_download: bool) -> Result<bool> {
        let responses = self.wait_search(search_id).await;
        let Some(mut w) = self.wishlist_item(id) else { return Ok(false) };
        w.last_match_count = responses.iter().map(|r| r.files.iter().filter(|f| file_matches(f, &w.filter)).count()).sum();
        let mut queued = false;
        if allow_download && w.auto_download && w.enabled {
            let items = pick_downloads(&responses, &w.filter);
            if !items.is_empty() {
                let transfers = self.add_downloads(items, None)?;
                tracing::info!("wishlist {:?}: queued {} files", w.query, transfers.len());
                w.downloaded_count += transfers.len();
                // found it: stop searching
                w.enabled = false;
                queued = true;
                if let Some(ext) = &w.external_id {
                    for t in &transfers {
                        self.db.put("lidarr_transfer", &t.id, ext);
                    }
                }
            }
        }
        self.save_wishlist_item(&w);
        Ok(queued)
    }

    pub(crate) async fn wishlist_loop(self: Arc<Self>) {
        tokio::time::sleep(Duration::from_secs(60)).await;
        loop {
            let (enabled, interval, max) = {
                let c = self.config.read();
                (c.wishlist.enabled, c.wishlist.interval_minutes.max(5), c.wishlist.max_auto_downloads_per_run)
            };
            if enabled && self.is_logged_in() {
                let _guard = self.wishlist_lock.lock().await;
                let mut downloads = 0usize;
                for w in self.wishlist().into_iter().filter(|w| w.enabled) {
                    if !self.is_logged_in() {
                        break;
                    }
                    let due = w.last_run_at.is_none_or(|t| chrono::Utc::now() - t >= chrono::Duration::minutes(interval as i64));
                    if !due {
                        continue;
                    }
                    let Ok(search) = self.start_search(&w.query) else { continue };
                    if self.update_wishlist_run(&w.id, &search.id).is_err() {
                        continue;
                    }
                    match self.finish_wishlist_run(&w.id, &search.id, max == 0 || downloads < max).await {
                        Ok(true) => downloads += 1,
                        Ok(false) => {}
                        Err(e) => tracing::warn!("wishlist {:?}: {e:#}", w.query),
                    }
                    // space searches out; the server dislikes bursts
                    tokio::time::sleep(Duration::from_secs(10)).await;
                }
            }
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, size: u64, ext: &str, br: Option<u32>) -> SearchFile {
        SearchFile { filename: name.into(), size, extension: ext.into(), bit_rate: br, ..Default::default() }
    }

    fn resp(user: &str, score: f64, files: Vec<SearchFile>) -> SearchResponse {
        SearchResponse {
            username: user.into(),
            has_free_upload_slot: true,
            upload_speed: 0,
            queue_length: 0,
            score,
            reputation: 0,
            files,
            locked_files: vec![],
        }
    }

    #[test]
    fn filters() {
        let f = WishlistFilter { extensions: vec![".mp3".into()], min_bit_rate: Some(256), min_size: None, max_size: None };
        assert!(file_matches(&file("a.mp3", 1, "mp3", Some(320)), &f));
        assert!(!file_matches(&file("a.mp3", 1, "mp3", Some(128)), &f));
        assert!(!file_matches(&file("a.flac", 1, "flac", None), &f));
        let f = WishlistFilter { extensions: vec![], min_bit_rate: Some(256), min_size: Some(10), max_size: Some(20) };
        assert!(file_matches(&file("a.flac", 15, "flac", None), &f));
        assert!(!file_matches(&file("a.flac", 5, "flac", None), &f));
    }

    #[test]
    fn picks_best_folder_with_alternates() {
        let rs = vec![
            resp("a", 1.0, vec![file("x\\Album\\01.flac", 100, "flac", None)]),
            resp("b", 1.0, vec![file("y\\Album\\01.flac", 100, "flac", None), file("y\\Album\\02.flac", 200, "flac", None)]),
            resp("c", 0.5, vec![file("z\\01.flac", 100, "flac", None)]),
        ];
        let items = pick_downloads(&rs, &WishlistFilter::default());
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|i| i.username == "b"));
        let first = items.iter().find(|i| i.filename.ends_with("01.flac")).unwrap();
        assert_eq!(first.sources.len(), 2);
        assert_eq!(first.sources[0].username, "a");
    }
}
