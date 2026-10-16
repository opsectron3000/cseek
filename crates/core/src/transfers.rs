//! Transfer bookkeeping shared by downloads and uploads.

use crate::client::Client;
use crate::downloads::{Chunk, DownloadJob};
use crate::model::*;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Notify;
use tokio::task::AbortHandle;

pub(crate) struct Entry {
    pub t: Transfer,
    pub destination: Option<String>,
    pub dirty: bool,
    pub task: Option<AbortHandle>,
    pub job: Option<Arc<DownloadJob>>,
    /// Chunk progress restored from the database, used when the download restarts.
    pub resume: Vec<Chunk>,
    pub started: Option<Instant>,
    /// Bytes at `started` (for resumed transfers) so speed reflects this session.
    pub start_bytes: u64,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Stored {
    pub transfer: Transfer,
    pub destination: Option<String>,
    #[serde(default)]
    pub chunks: Vec<Chunk>,
}

pub(crate) struct Transfers {
    pub inner: Mutex<Inner>,
    pub upload_notify: Notify,
    /// Per-group upload speed limiters, keyed by group name.
    pub group_limiters: Mutex<HashMap<String, Arc<crate::net::Limiter>>>,
}

#[derive(Default)]
pub(crate) struct Inner {
    pub map: HashMap<String, Entry>,
    pub order: Vec<String>,
}

impl Inner {
    pub fn insert(&mut self, e: Entry) {
        self.order.push(e.t.id.clone());
        self.map.insert(e.t.id.clone(), e);
    }
    pub fn remove(&mut self, id: &str) -> Option<Entry> {
        self.order.retain(|x| x != id);
        self.map.remove(id)
    }
    pub fn iter(&self) -> impl Iterator<Item = &Entry> {
        self.order.iter().filter_map(|id| self.map.get(id))
    }
}

impl Transfers {
    pub fn new() -> Self {
        Self { inner: Mutex::new(Inner::default()), upload_notify: Notify::new(), group_limiters: Mutex::new(HashMap::new()) }
    }

    pub fn usernames(&self) -> Vec<String> {
        let inner = self.inner.lock();
        let mut v: Vec<String> = inner.iter().filter(|e| !e.t.state.is_finished()).map(|e| e.t.username.clone()).collect();
        v.sort();
        v.dedup();
        v
    }

    pub fn has_active(&self) -> bool {
        self.inner.lock().iter().any(|e| e.t.state == TransferState::InProgress)
    }
}

impl Client {
    pub(crate) fn load_transfers(&self) {
        let stored: Vec<Stored> = self.db.all("transfer");
        let mut inner = self.transfers.inner.lock();
        for s in stored {
            let mut t = s.transfer;
            if !t.state.is_finished() {
                // in-flight work restarts; uploads must be re-requested by the peer, downloads resume
                match t.direction {
                    Direction::Download => {
                        t.state = TransferState::Queued;
                        t.average_speed = 0.0;
                        for src in &mut t.sources {
                            if src.state != SourceState::Failed {
                                src.state = SourceState::Idle;
                            }
                        }
                    }
                    Direction::Upload => {
                        if t.state != TransferState::Queued {
                            t.state = TransferState::Queued;
                        }
                    }
                }
            }
            inner.insert(Entry {
                t,
                destination: s.destination,
                dirty: false,
                task: None,
                job: None,
                resume: s.chunks,
                started: None,
                start_bytes: 0,
            });
        }
    }

    pub(crate) fn persist_transfer(&self, id: &str) {
        let stored = {
            let inner = self.transfers.inner.lock();
            inner.map.get(id).map(|e| Stored {
                transfer: e.t.clone(),
                destination: e.destination.clone(),
                chunks: e.job.as_ref().map(|j| j.chunks_snapshot()).unwrap_or_else(|| e.resume.clone()),
            })
        };
        if let Some(s) = stored {
            self.db.put("transfer", id, &s);
        }
    }

    pub fn persist_active_transfers(&self) {
        let ids: Vec<String> =
            self.transfers.inner.lock().iter().filter(|e| e.t.state == TransferState::InProgress).map(|e| e.t.id.clone()).collect();
        for id in ids {
            self.persist_transfer(&id);
        }
    }

    /// Mutate a transfer; marks it dirty for the next progress flush, or emits immediately if `now`.
    pub(crate) fn update_transfer(&self, id: &str, now: bool, f: impl FnOnce(&mut Entry)) -> Option<Transfer> {
        let t = {
            let mut inner = self.transfers.inner.lock();
            let e = inner.map.get_mut(id)?;
            let before = e.t.state;
            f(e);
            if e.t.state == TransferState::InProgress && before != TransferState::InProgress {
                e.started = Some(Instant::now());
                e.start_bytes = e.t.bytes_transferred;
                e.t.started_at.get_or_insert(chrono::Utc::now());
            }
            if e.t.state.is_finished() && !before.is_finished() {
                e.t.ended_at = Some(chrono::Utc::now());
            }
            if let Some(st) = e.started {
                let secs = st.elapsed().as_secs_f64();
                if secs > 0.5 {
                    e.t.average_speed = (e.t.bytes_transferred.saturating_sub(e.start_bytes)) as f64 / secs;
                }
            }
            e.dirty = !now;
            e.t.clone()
        };
        if now {
            self.persist_transfer(id);
            self.emit(Event::TransferUpdated(Box::new(t.clone())));
        }
        Some(t)
    }

    pub(crate) fn flush_transfer_events(&self) {
        let dirty: Vec<Transfer> = {
            let mut inner = self.transfers.inner.lock();
            inner
                .map
                .values_mut()
                .filter(|e| e.dirty)
                .map(|e| {
                    e.dirty = false;
                    if let Some(st) = e.started {
                        if e.t.state == TransferState::InProgress {
                            let secs = st.elapsed().as_secs_f64().max(0.001);
                            e.t.average_speed = (e.t.bytes_transferred.saturating_sub(e.start_bytes)) as f64 / secs;
                        }
                    }
                    e.t.clone()
                })
                .collect()
        };
        for t in dirty {
            self.emit(Event::TransferUpdated(Box::new(t)));
        }
    }

    pub fn transfers(&self, direction: Direction) -> Vec<Transfer> {
        self.transfers.inner.lock().iter().filter(|e| e.t.direction == direction).map(|e| e.t.clone()).collect()
    }

    pub fn transfer(&self, id: &str) -> Option<Transfer> {
        self.transfers.inner.lock().map.get(id).map(|e| e.t.clone())
    }

    pub(crate) fn transfer_summary(&self) -> TransferSummary {
        let inner = self.transfers.inner.lock();
        let mut s = TransferSummary {
            download_speed: self.download_meter.sample(),
            upload_speed: self.upload_meter.sample(),
            active_downloads: 0,
            active_uploads: 0,
            queued_uploads: 0,
        };
        for e in inner.iter() {
            match (e.t.direction, e.t.state) {
                (Direction::Download, TransferState::InProgress) => s.active_downloads += 1,
                (Direction::Upload, TransferState::InProgress | TransferState::Requested) => s.active_uploads += 1,
                (Direction::Upload, TransferState::Queued) => s.queued_uploads += 1,
                _ => {}
            }
        }
        s
    }

    /// Cancel a transfer; optionally forget it.
    pub fn cancel_transfer(self: &Arc<Self>, id: &str, remove: bool) -> bool {
        let entry = {
            let mut inner = self.transfers.inner.lock();
            let Some(e) = inner.map.get_mut(id) else { return false };
            if let Some(t) = e.task.take() {
                t.abort();
            }
            if let Some(j) = &e.job {
                j.cancel();
            }
            let was_active = !e.t.state.is_finished();
            if was_active {
                e.t.state = TransferState::Cancelled;
                e.t.ended_at = Some(chrono::Utc::now());
            }
            (e.t.clone(), was_active)
        };
        let (t, was_active) = entry;
        if was_active && t.direction == Direction::Upload {
            let me = self.clone();
            let (u, f) = (t.username.clone(), t.filename.clone());
            tokio::spawn(async move {
                let _ = me.peer_send(&u, slsk_proto::PeerMessage::UploadDenied { filename: f, reason: "Cancelled".into() }).await;
            });
            self.notify_upload_scheduler();
        }
        if remove {
            self.transfers.inner.lock().remove(id);
            self.db.delete("transfer", id);
            if t.direction == Direction::Download {
                let _ = std::fs::remove_file(self.part_path(id));
            }
            self.emit(Event::TransferRemoved { id: id.to_owned() });
        } else {
            self.persist_transfer(id);
            self.emit(Event::TransferUpdated(Box::new(t)));
        }
        true
    }

    /// Remove finished transfers. `only_succeeded` limits to successful ones.
    pub fn clear_transfers(self: &Arc<Self>, direction: Direction, only_succeeded: bool) {
        let ids: Vec<String> = self
            .transfers
            .inner
            .lock()
            .iter()
            .filter(|e| e.t.direction == direction)
            .filter(|e| if only_succeeded { e.t.state == TransferState::Succeeded } else { e.t.state.is_finished() })
            .map(|e| e.t.id.clone())
            .collect();
        for id in ids {
            self.cancel_transfer(&id, true);
        }
    }

    /// slskdn: blacklisting a user cancels their transfers immediately.
    pub fn cancel_user_transfers(self: &Arc<Self>, username: &str, reason: &str) {
        let ids: Vec<String> = self
            .transfers
            .inner
            .lock()
            .iter()
            .filter(|e| !e.t.state.is_finished() && e.t.username == username)
            .map(|e| e.t.id.clone())
            .collect();
        for id in &ids {
            self.cancel_transfer(id, false);
            self.update_transfer(id, true, |e| e.t.state_detail = Some(reason.to_owned()));
        }
        if !ids.is_empty() {
            tracing::info!("cancelled {} transfer(s) with {username}: {reason}", ids.len());
        }
    }

    pub(crate) fn on_user_status_changed(self: &Arc<Self>, username: &str, status: UserStatus) {
        tracing::trace!("{username} is now {status:?}");
        let _ = status;
    }
}
