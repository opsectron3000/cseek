//! Uploads: request validation, the priority/slot scheduler and the file sender.

use crate::client::Client;
use crate::model::*;
use crate::net::Limiter;
use crate::transfers::Entry;
use anyhow::{anyhow, bail, Result};
use slsk_proto::init::CONN_FILE;
use slsk_proto::peer::DIRECTION_UPLOAD;
use slsk_proto::PeerMessage;
use std::collections::HashMap;
use std::io::SeekFrom;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

/// Reason strings understood by other clients.
const NOT_SHARED: &str = "File not shared.";
const BANNED: &str = "Banned";
const TOO_MANY_FILES: &str = "Too many files";
const TOO_MANY_BYTES: &str = "Too many megabytes";

impl Client {
    pub(crate) fn notify_upload_scheduler(&self) {
        self.transfers.upload_notify.notify_one();
    }

    /// (active, queued) uploads.
    pub(crate) fn upload_counts(&self) -> (usize, usize) {
        let inner = self.transfers.inner.lock();
        let mut active = 0;
        let mut queued = 0;
        for e in inner.iter().filter(|e| e.t.direction == Direction::Upload) {
            match e.t.state {
                TransferState::Requested | TransferState::InProgress => active += 1,
                TransferState::Queued => queued += 1,
                _ => {}
            }
        }
        (active, queued)
    }

    pub(crate) async fn on_queue_upload(self: &Arc<Self>, username: &str, filename: String) {
        if let Err(reason) = self.enqueue_upload(username, &filename) {
            let _ = self.peer_send(username, PeerMessage::UploadDenied { filename, reason }).await;
        }
    }

    /// Validate and queue an upload request. `Err` carries the denial reason.
    pub(crate) fn enqueue_upload(self: &Arc<Self>, username: &str, filename: &str) -> std::result::Result<(), String> {
        if self.is_blacklisted(username) || self.guard.is_banned(username) {
            return Err(BANNED.into());
        }
        let per_min = self.config.read().guard.max_queue_requests_per_minute;
        if !self.guard.allow_rate(username, "queue-flood", per_min) {
            self.record_violation(username, "queue-flood");
            return Err("Too many requests".into());
        }
        let ix = self.shares.snapshot();
        let Some(file) = ix.get(filename) else {
            if !self.guard.allow_rate(username, "not-shared", 30) {
                self.record_violation(username, "not-shared-scan");
            }
            return Err(NOT_SHARED.into());
        };
        let group = self.user_group_info(username);
        if let Some(allowed) = &group.allowed_extensions {
            if !allowed.contains(&file.ext) {
                return Err(NOT_SHARED.into());
            }
        }
        let (max_files, max_bytes) = {
            let c = self.config.read();
            let files = if group.queue_limit_files > 0 { group.queue_limit_files } else { c.transfers.max_queued_files_per_user };
            (files, c.transfers.max_queued_bytes_per_user)
        };
        let t = {
            let mut inner = self.transfers.inner.lock();
            let mut n = 0u32;
            let mut bytes = 0u64;
            for e in inner.iter().filter(|e| e.t.direction == Direction::Upload && e.t.username == username && !e.t.state.is_finished()) {
                if e.t.filename == file.vpath {
                    // already queued: nothing to do
                    return Ok(());
                }
                n += 1;
                bytes += e.t.size;
            }
            if max_files > 0 && n >= max_files {
                return Err(TOO_MANY_FILES.into());
            }
            if max_bytes > 0 && bytes + file.size > max_bytes {
                return Err(TOO_MANY_BYTES.into());
            }
            // a finished upload of the same file is replaced by the new request
            let old: Vec<String> = inner
                .iter()
                .filter(|e| e.t.direction == Direction::Upload && e.t.username == username && e.t.filename == file.vpath)
                .map(|e| e.t.id.clone())
                .collect();
            for id in &old {
                inner.remove(id);
                self.db.delete("transfer", id);
            }
            let t = Transfer {
                id: uuid::Uuid::new_v4().to_string(),
                direction: Direction::Upload,
                username: username.to_owned(),
                filename: file.vpath.clone(),
                size: file.size,
                state: TransferState::Queued,
                state_detail: None,
                bytes_transferred: 0,
                average_speed: 0.0,
                place_in_queue: None,
                requested_at: chrono::Utc::now(),
                started_at: None,
                ended_at: None,
                local_path: Some(file.local.to_string_lossy().into_owned()),
                sources: vec![],
            };
            inner.insert(Entry {
                t: t.clone(),
                destination: None,
                dirty: false,
                task: None,
                job: None,
                resume: vec![],
                started: None,
                start_bytes: 0,
            });
            for id in old {
                self.emit(Event::TransferRemoved { id });
            }
            t
        };
        tracing::debug!("{username} queued {}", t.filename);
        self.persist_transfer(&t.id);
        self.emit(Event::TransferUpdated(Box::new(t)));
        self.watch_user(username);
        self.notify_upload_scheduler();
        Ok(())
    }

    /// Queued uploads in service order.
    fn upload_queue(&self) -> Vec<(String, String, u32)> {
        let entries: Vec<(String, String, chrono::DateTime<chrono::Utc>)> = self
            .transfers
            .inner
            .lock()
            .iter()
            .filter(|e| e.t.direction == Direction::Upload && e.t.state == TransferState::Queued && e.task.is_none())
            .map(|e| (e.t.id.clone(), e.t.username.clone(), e.t.requested_at))
            .collect();
        let mut prio: HashMap<String, u32> = HashMap::new();
        let mut v: Vec<(String, String, u32, chrono::DateTime<chrono::Utc>)> = entries
            .into_iter()
            .map(|(id, u, at)| {
                let p = *prio.entry(u.clone()).or_insert_with(|| self.user_group_info(&u).priority);
                (id, u, p, at)
            })
            .collect();
        v.sort_by(|a, b| a.2.cmp(&b.2).then(a.3.cmp(&b.3)));
        v.into_iter().map(|(id, u, p, _)| (id, u, p)).collect()
    }

    pub(crate) async fn on_place_in_queue_request(self: &Arc<Self>, username: &str, filename: String) {
        let q = self.upload_queue();
        let ids: Vec<String> = q.iter().map(|x| x.0.clone()).collect();
        let place = {
            let inner = self.transfers.inner.lock();
            ids.iter()
                .position(|id| inner.map.get(id).is_some_and(|e| e.t.username == username && e.t.filename == filename))
                .map(|p| p as u32 + 1)
        };
        if let Some(place) = place {
            let _ = self.peer_send(username, PeerMessage::PlaceInQueueResponse { filename, place }).await;
        }
    }

    pub(crate) async fn upload_scheduler(self: Arc<Self>) {
        loop {
            tokio::select! {
                _ = self.transfers.upload_notify.notified() => {}
                _ = tokio::time::sleep(Duration::from_secs(3)) => {}
            }
            if self.is_logged_in() {
                self.schedule_uploads();
            }
        }
    }

    fn schedule_uploads(self: &Arc<Self>) {
        let slots = self.config.read().transfers.upload_slots.max(1) as usize;
        let (mut active_total, mut active_users, mut active_groups) = {
            let inner = self.transfers.inner.lock();
            let mut users: HashMap<String, usize> = HashMap::new();
            let mut total = 0;
            for e in inner.iter().filter(|e| {
                e.t.direction == Direction::Upload && matches!(e.t.state, TransferState::Requested | TransferState::InProgress)
            }) {
                total += 1;
                *users.entry(e.t.username.clone()).or_default() += 1;
            }
            (total, users, HashMap::<String, usize>::new())
        };
        if active_total >= slots {
            return;
        }
        for u in active_users.keys() {
            *active_groups.entry(self.user_group_info(u).name).or_default() += 1;
        }
        for (id, username, _) in self.upload_queue() {
            if active_total >= slots {
                break;
            }
            if active_users.contains_key(&username) {
                continue; // one upload per user at a time
            }
            let g = self.user_group_info(&username);
            let in_group = active_groups.get(&g.name).copied().unwrap_or(0);
            if g.upload_slots > 0 && in_group >= g.upload_slots as usize {
                continue;
            }
            active_total += 1;
            active_users.insert(username.clone(), 1);
            *active_groups.entry(g.name.clone()).or_default() += 1;
            let me = self.clone();
            let mut inner = self.transfers.inner.lock();
            if let Some(e) = inner.map.get_mut(&id) {
                e.t.state = TransferState::Requested;
                let id2 = id.clone();
                let h = tokio::spawn(async move { me.run_upload(id2).await });
                e.task = Some(h.abort_handle());
            }
        }
    }

    async fn run_upload(self: Arc<Self>, id: String) {
        self.update_transfer(&id, true, |_| {});
        let result = self.upload_inner(&id).await;
        let t = match result {
            Ok(()) => {
                self.counters.uploads_succeeded.fetch_add(1, Ordering::Relaxed);
                self.update_transfer(&id, true, |e| {
                    e.t.state = TransferState::Succeeded;
                    e.t.state_detail = None;
                })
            }
            Err(e) => {
                let msg = format!("{e:#}");
                let requeue = msg.contains("Queued");
                if !requeue {
                    self.counters.uploads_failed.fetch_add(1, Ordering::Relaxed);
                }
                let t = self.update_transfer(&id, true, |e| {
                    if requeue {
                        e.t.state = TransferState::Queued;
                    } else {
                        e.t.state = if msg.starts_with("rejected") { TransferState::Rejected } else { TransferState::Failed };
                        e.t.state_detail = Some(msg.clone());
                    }
                });
                if let Some(t) = &t {
                    if !requeue && t.bytes_transferred > 0 {
                        let me = self.clone();
                        let (u, f) = (t.username.clone(), t.filename.clone());
                        tokio::spawn(async move {
                            let _ = me.peer_send(&u, PeerMessage::UploadFailed { filename: f }).await;
                        });
                    }
                }
                t
            }
        };
        if let Some(t) = t {
            if t.state == TransferState::Succeeded {
                let bytes = t.size;
                self.record_reputation(&t.username, |r| {
                    r.uploads_succeeded += 1;
                    r.bytes_uploaded += bytes;
                });
            }
        }
        if let Some(e) = self.transfers.inner.lock().map.get_mut(&id) {
            e.task = None;
        }
        self.notify_upload_scheduler();
    }

    fn group_limiter(&self, group: &str, kbps: u64) -> Option<Arc<Limiter>> {
        if kbps == 0 {
            return None;
        }
        let mut m = self.transfers.group_limiters.lock();
        let l = m.entry(group.to_owned()).or_insert_with(|| Arc::new(Limiter::new(kbps * 1024)));
        l.set_rate(kbps * 1024);
        Some(l.clone())
    }

    async fn upload_inner(self: &Arc<Self>, id: &str) -> Result<()> {
        let t = self.transfer(id).ok_or_else(|| anyhow!("transfer removed"))?;
        let ix = self.shares.snapshot();
        let file = ix.get(&t.filename).cloned().ok_or_else(|| anyhow!("{NOT_SHARED}"))?;
        drop(ix);
        let u = t.username.clone();
        let token = self.token();
        let key = format!("tresp:{u}:{token}");
        let reply = self
            .peer_request(
                &u,
                PeerMessage::TransferRequest { direction: DIRECTION_UPLOAD, token, filename: t.filename.clone(), size: Some(file.size) },
                key,
                Duration::from_secs(60),
            )
            .await?;
        match reply {
            PeerMessage::TransferResponse { allowed: true, .. } => {}
            PeerMessage::TransferResponse { allowed: false, reason, .. } => {
                let r = reason.unwrap_or_else(|| "Cancelled".into());
                if r == "Queued" {
                    bail!("Queued");
                }
                bail!("rejected: {r}");
            }
            _ => bail!("unexpected reply"),
        }
        let mut stream = self.connect_peer(&u, CONN_FILE, None).await?;
        stream.write_all(&token.to_le_bytes()).await?;
        let mut off = [0u8; 8];
        tokio::time::timeout(Duration::from_secs(60), stream.read_exact(&mut off))
            .await
            .map_err(|_| anyhow!("peer sent no offset"))??;
        let offset = u64::from_le_bytes(off);
        if offset > file.size {
            bail!("invalid offset {offset}");
        }
        let mut f = tokio::fs::File::open(&file.local).await?;
        f.seek(SeekFrom::Start(offset)).await?;
        self.update_transfer(id, true, |e| {
            e.t.state = TransferState::InProgress;
            e.t.bytes_transferred = offset;
        });
        let group = self.user_group_info(&u);
        let glimit = self.group_limiter(&group.name, group.speed_limit_kbps);
        let mut sent = offset;
        let mut buf = vec![0u8; 32 * 1024];
        while sent < file.size {
            let n = f.read(&mut buf).await?;
            if n == 0 {
                bail!("file shrank while uploading");
            }
            self.upload_limiter.acquire(n).await;
            if let Some(l) = &glimit {
                l.acquire(n).await;
            }
            tokio::time::timeout(Duration::from_secs(120), stream.write_all(&buf[..n]))
                .await
                .map_err(|_| anyhow!("peer stopped reading"))??;
            sent += n as u64;
            self.upload_meter.add(n as u64);
            self.update_transfer(id, false, |e| e.t.bytes_transferred = sent);
        }
        stream.flush().await?;
        // Give the downloader a moment to close first, so the final bytes are not reset.
        let mut sink = [0u8; 64];
        let _ = tokio::time::timeout(Duration::from_secs(10), stream.read(&mut sink)).await;
        Ok(())
    }
}
