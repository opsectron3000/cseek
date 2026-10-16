//! Outgoing searches and answering other users' searches.

use crate::client::Client;
use crate::model::*;
use anyhow::{bail, Result};
use parking_lot::Mutex;
use slsk_proto::peer::SearchResponse as WireResponse;
use slsk_proto::{PeerMessage, ServerRequest};
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::watch;

const KEEP_IN_MEMORY: usize = 20;
const MAX_SEARCH_DURATION: Duration = Duration::from_secs(300);

pub(crate) struct SearchEntry {
    pub search: Search,
    pub token: u32,
    /// `None` when evicted from memory (responses live in the database).
    pub responses: Option<Vec<SearchResponse>>,
    pub dirty: bool,
    pub last_response: Instant,
    pub done: watch::Sender<bool>,
}

#[derive(Default)]
pub(crate) struct Searches {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    list: Vec<SearchEntry>,
    by_token: HashMap<u32, String>,
}

/// Smart ranking: free slot, speed, queue depth and local reputation.
pub fn score_response(free: bool, speed: u32, queue: u32, reputation: i32) -> f64 {
    let speed_n = (speed as f64 / (2.0 * 1024.0 * 1024.0)).min(1.0);
    let queue_n = 1.0 - (queue.min(50) as f64 / 50.0);
    let rep_n = (reputation + 100) as f64 / 200.0;
    let s = if free { 40.0 } else { 0.0 } + 30.0 * speed_n.sqrt() + 20.0 * queue_n + 10.0 * rep_n;
    (s * 10.0).round() / 10.0
}

impl Client {
    pub(crate) fn load_searches(&self) {
        let mut list: Vec<Search> = self.db.all("search");
        list.sort_by_key(|s| s.started_at);
        let mut inner = self.searches.inner.lock();
        for mut s in list {
            if s.state == SearchState::InProgress {
                s.state = SearchState::Completed;
                s.ended_at.get_or_insert(chrono::Utc::now());
            }
            let (done, _) = watch::channel(true);
            inner.list.push(SearchEntry { search: s, token: 0, responses: None, dirty: false, last_response: Instant::now(), done });
        }
    }

    pub fn searches(&self) -> Vec<Search> {
        let inner = self.searches.inner.lock();
        inner.list.iter().rev().map(|e| e.search.clone()).collect()
    }

    pub fn search(&self, id: &str) -> Option<Search> {
        self.searches.inner.lock().list.iter().find(|e| e.search.id == id).map(|e| e.search.clone())
    }

    pub fn search_responses(&self, id: &str) -> Option<Vec<SearchResponse>> {
        let mem = {
            let inner = self.searches.inner.lock();
            let e = inner.list.iter().find(|e| e.search.id == id)?;
            e.responses.clone()
        };
        let mut r = mem.or_else(|| self.db.get::<Vec<SearchResponse>>("search_responses", id)).unwrap_or_default();
        r.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        Some(r)
    }

    pub fn start_search(self: &Arc<Self>, query: &str) -> Result<Search> {
        let query = query.trim().to_owned();
        if query.is_empty() {
            bail!("query is empty");
        }
        if !self.is_logged_in() {
            bail!("not connected to the Soulseek server");
        }
        let token = self.token();
        let search = Search {
            id: uuid::Uuid::new_v4().to_string(),
            query: query.clone(),
            state: SearchState::InProgress,
            started_at: chrono::Utc::now(),
            ended_at: None,
            response_count: 0,
            file_count: 0,
            locked_file_count: 0,
        };
        self.server_send(ServerRequest::FileSearch { token, query })?;
        self.counters.searches_sent.fetch_add(1, Ordering::Relaxed);
        let (done, _) = watch::channel(false);
        {
            let mut inner = self.searches.inner.lock();
            inner.by_token.insert(token, search.id.clone());
            inner.list.push(SearchEntry {
                search: search.clone(),
                token,
                responses: Some(vec![]),
                dirty: false,
                last_response: Instant::now(),
                done,
            });
            // evict old in-memory responses
            let n = inner.list.len();
            if n > KEEP_IN_MEMORY {
                for e in inner.list[..n - KEEP_IN_MEMORY].iter_mut() {
                    e.responses = None;
                }
            }
        }
        self.db.put("search", &search.id, &search);
        self.emit(Event::SearchUpdated(search.clone()));
        let me = self.clone();
        let id = search.id.clone();
        tokio::spawn(async move { me.search_timer(id).await });
        Ok(search)
    }

    async fn search_timer(self: Arc<Self>, id: String) {
        let started = Instant::now();
        let timeout = Duration::from_secs(self.config.read().search.timeout_seconds.max(3));
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let (limit_r, limit_f) = {
                let c = self.config.read();
                (c.search.response_limit, c.search.file_limit)
            };
            let finish = {
                let inner = self.searches.inner.lock();
                let Some(e) = inner.list.iter().find(|e| e.search.id == id) else { return };
                if e.search.state != SearchState::InProgress {
                    return;
                }
                let quiet = e.last_response.elapsed() >= timeout && started.elapsed() >= timeout;
                quiet
                    || e.search.response_count >= limit_r
                    || e.search.file_count >= limit_f
                    || started.elapsed() >= MAX_SEARCH_DURATION
            };
            if finish {
                self.finish_search(&id, SearchState::Completed);
                return;
            }
        }
    }

    fn finish_search(&self, id: &str, state: SearchState) -> Option<Search> {
        let (search, responses) = {
            let mut inner = self.searches.inner.lock();
            let e = inner.list.iter_mut().find(|e| e.search.id == id)?;
            if e.search.state != SearchState::InProgress {
                return Some(e.search.clone());
            }
            e.search.state = state;
            e.search.ended_at = Some(chrono::Utc::now());
            e.dirty = false;
            let _ = e.done.send(true);
            let token = e.token;
            let out = (e.search.clone(), e.responses.clone());
            inner.by_token.remove(&token);
            out
        };
        self.db.put("search", id, &search);
        if let Some(r) = responses {
            self.db.put("search_responses", id, &r);
        }
        self.emit(Event::SearchUpdated(search.clone()));
        Some(search)
    }

    pub fn stop_search(&self, id: &str) -> Option<Search> {
        self.finish_search(id, SearchState::Cancelled)
    }

    pub fn delete_search(&self, id: &str) {
        let removed = {
            let mut inner = self.searches.inner.lock();
            let before = inner.list.len();
            if let Some(e) = inner.list.iter().find(|e| e.search.id == id) {
                let _ = e.done.send(true);
                let t = e.token;
                inner.by_token.remove(&t);
            }
            inner.list.retain(|e| e.search.id != id);
            before != inner.list.len()
        };
        if removed {
            self.db.delete("search", id);
            self.db.delete("search_responses", id);
            self.emit(Event::SearchRemoved { id: id.to_owned() });
        }
    }

    pub fn clear_searches(&self) {
        let ids: Vec<String> = self.searches.inner.lock().list.iter().map(|e| e.search.id.clone()).collect();
        for id in ids {
            self.delete_search(&id);
        }
    }

    /// Wait until a search finishes; returns its responses.
    pub async fn wait_search(&self, id: &str) -> Vec<SearchResponse> {
        let rx = {
            let inner = self.searches.inner.lock();
            inner.list.iter().find(|e| e.search.id == id).map(|e| e.done.subscribe())
        };
        if let Some(mut rx) = rx {
            let _ = tokio::time::timeout(MAX_SEARCH_DURATION + Duration::from_secs(5), rx.wait_for(|d| *d)).await;
        }
        self.search_responses(id).unwrap_or_default()
    }

    pub(crate) fn on_search_response(&self, r: WireResponse) {
        if self.is_blacklisted(&r.username) || self.guard.is_banned(&r.username) {
            return;
        }
        let reputation = self.reputation(&r.username).score;
        let response = SearchResponse {
            username: r.username.clone(),
            has_free_upload_slot: r.slot_free,
            upload_speed: r.avg_speed,
            queue_length: r.queue_length,
            score: score_response(r.slot_free, r.avg_speed, r.queue_length, reputation),
            reputation,
            files: r.files.iter().map(SearchFile::from).collect(),
            locked_files: r.private_files.iter().map(SearchFile::from).collect(),
        };
        let mut inner = self.searches.inner.lock();
        let Some(id) = inner.by_token.get(&r.token).cloned() else { return };
        let Some(e) = inner.list.iter_mut().find(|e| e.search.id == id) else { return };
        if e.search.state != SearchState::InProgress {
            return;
        }
        let list = e.responses.get_or_insert_with(Vec::new);
        if list.iter().any(|x| x.username == response.username) {
            return;
        }
        e.search.response_count += 1;
        e.search.file_count += response.files.len();
        e.search.locked_file_count += response.locked_files.len();
        list.push(response);
        e.dirty = true;
        e.last_response = Instant::now();
    }

    pub(crate) fn flush_search_events(&self) {
        let dirty: Vec<Search> = {
            let mut inner = self.searches.inner.lock();
            inner
                .list
                .iter_mut()
                .filter(|e| e.dirty)
                .map(|e| {
                    e.dirty = false;
                    e.search.clone()
                })
                .collect()
        };
        for s in dirty {
            self.emit(Event::SearchUpdated(s));
        }
    }

    /// A search request from another user (via server, distributed parent, or embedded).
    pub(crate) fn handle_search_request(self: &Arc<Self>, username: String, token: u32, query: String) {
        self.counters.search_requests_received.fetch_add(1, Ordering::Relaxed);
        if Some(&username) == self.username().as_ref() || username.is_empty() {
            return;
        }
        if !self.may_browse(&username) {
            return;
        }
        let lower = query.to_lowercase();
        {
            let server_ex = self.excluded_phrases.lock();
            let cfg = self.config.read();
            if server_ex.iter().any(|p| !p.is_empty() && lower.contains(p.as_str()))
                || cfg.search.excluded_phrases.iter().any(|p| !p.is_empty() && lower.contains(&p.to_lowercase()))
            {
                return;
            }
        }
        let limit = self.config.read().search.max_results_per_response;
        let ix = self.shares.snapshot();
        let allowed = self.allowed_extensions_for(&username);
        let files: Vec<slsk_proto::FileEntry> = ix
            .search(&query, limit)
            .into_iter()
            .filter(|f| allowed.as_ref().is_none_or(|a| a.contains(&f.ext)))
            .map(|f| slsk_proto::FileEntry {
                filename: f.vpath.clone(),
                size: f.size,
                extension: f.ext.clone(),
                attributes: f.attrs.clone(),
            })
            .collect();
        if files.is_empty() {
            return;
        }
        if !self.guard.allow_rate(&username, "search-response", self.config.read().guard.max_search_responses_per_minute) {
            return;
        }
        let me = self.clone();
        tokio::spawn(async move {
            let slots = me.config.read().transfers.upload_slots;
            let (active, queued) = me.upload_counts();
            let resp = WireResponse {
                username: me.username().unwrap_or_default(),
                token,
                files,
                slot_free: (active as u32) < slots,
                avg_speed: me.upload_meter.sample() as u32,
                queue_length: queued as u32,
                private_files: vec![],
            };
            match me.peer_send(&username, PeerMessage::FileSearchResponse(resp)).await {
                Ok(()) => {
                    me.counters.search_responses_sent.fetch_add(1, Ordering::Relaxed);
                }
                Err(e) => tracing::trace!("search response to {username} failed: {e}"),
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoring_prefers_free_fast_short_queue() {
        let good = score_response(true, 2_000_000, 0, 50);
        let bad = score_response(false, 10_000, 40, -50);
        assert!(good > bad);
        assert!(good <= 100.0);
    }
}
