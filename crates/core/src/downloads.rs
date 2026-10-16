//! Downloads: chunked, multi-source transfers with failover and auto-replacement
//! of stalled sources.
//!
//! The Soulseek protocol only lets a downloader choose the start offset, so a chunk
//! is fetched by queueing the file, sending the chunk's offset when the uploader
//! connects, and closing the connection once the chunk is complete. A source that
//! finishes a chunk keeps streaming into the next one if nobody else owns it, so a
//! single source downloads the whole file in one session.

use crate::client::{Client, Waited};
use crate::model::*;
use crate::transfers::Entry;
use anyhow::{anyhow, bail, Result};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use slsk_proto::peer::DIRECTION_UPLOAD;
use slsk_proto::PeerMessage;
use std::collections::{HashMap, HashSet};
use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::{oneshot, Notify};
use tokio::task::JoinSet;

const MAX_PARALLEL_SOURCES: usize = 4;
const MAX_SOURCE_ATTEMPTS: u32 = 3;
const MAX_ALTERNATES_PER_SEARCH: usize = 5;
const REPLACE_INTERVAL: Duration = Duration::from_secs(600);
const PLACE_REFRESH: Duration = Duration::from_secs(300);
const MIN_CHUNK: u64 = 1024 * 1024;
/// Remote denial reasons that will not change by retrying.
const PERMANENT_DENIALS: [&str; 4] = ["file not shared.", "banned", "blocked country", "file read error."];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct Chunk {
    pub start: u64,
    pub end: u64,
    pub done: u64,
}

impl Chunk {
    fn pos(&self) -> u64 {
        self.start + self.done
    }
    fn complete(&self) -> bool {
        self.pos() >= self.end
    }
}

struct ChunkRt {
    c: Chunk,
    owner: Option<usize>,
}

pub(crate) struct DownloadJob {
    pub id: String,
    /// 0 until known: a download added without a size learns it from the peer's TransferRequest.
    size: AtomicU64,
    size_known: AtomicBool,
    chunk_size: u64,
    pub part_path: PathBuf,
    chunks: Mutex<Vec<ChunkRt>>,
    cancelled: AtomicBool,
    changed: Notify,
    last_progress: Mutex<Instant>,
}

fn layout(size: u64, chunk: u64) -> Vec<Chunk> {
    if size == 0 {
        return vec![Chunk { start: 0, end: 0, done: 0 }];
    }
    let chunk = chunk.max(MIN_CHUNK);
    let mut v = Vec::new();
    let mut start = 0;
    while start < size {
        let end = (start + chunk).min(size);
        v.push(Chunk { start, end, done: 0 });
        start = end;
    }
    v
}

fn valid_layout(chunks: &[Chunk], size: u64) -> bool {
    !chunks.is_empty()
        && chunks[0].start == 0
        && chunks.last().map(|c| c.end) == Some(size)
        && chunks.windows(2).all(|w| w[0].end == w[1].start)
        && chunks.iter().all(|c| c.start <= c.end && c.done <= c.end - c.start)
}

impl DownloadJob {
    fn new(id: &str, size: u64, part_path: PathBuf, chunk_size: u64, resume: Vec<Chunk>) -> Self {
        let chunks = if valid_layout(&resume, size) { resume } else { layout(size, chunk_size) };
        Self {
            id: id.to_owned(),
            size: AtomicU64::new(size),
            size_known: AtomicBool::new(size > 0),
            chunk_size,
            part_path,
            chunks: Mutex::new(chunks.into_iter().map(|c| ChunkRt { c, owner: None }).collect()),
            cancelled: AtomicBool::new(false),
            changed: Notify::new(),
            last_progress: Mutex::new(Instant::now()),
        }
    }

    pub fn size(&self) -> u64 {
        self.size.load(Ordering::SeqCst)
    }

    /// Adopt the size the peer reported when ours was unknown. Returns true if it changed.
    fn learn_size(&self, size: u64) -> bool {
        if self.size_known.swap(true, Ordering::SeqCst) {
            return false;
        }
        self.size.store(size, Ordering::SeqCst);
        *self.chunks.lock() = layout(size, self.chunk_size).into_iter().map(|c| ChunkRt { c, owner: None }).collect();
        self.changed.notify_waiters();
        true
    }

    pub fn chunks_snapshot(&self) -> Vec<Chunk> {
        self.chunks.lock().iter().map(|c| c.c.clone()).collect()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.changed.notify_waiters();
    }

    fn reset_progress(&self) {
        for c in self.chunks.lock().iter_mut() {
            c.c.done = 0;
        }
    }

    fn done_bytes(&self) -> u64 {
        self.chunks.lock().iter().map(|c| c.c.done).sum()
    }

    fn chunk_count(&self) -> usize {
        self.chunks.lock().len()
    }

    fn is_complete(&self) -> bool {
        self.size_known.load(Ordering::SeqCst) && self.chunks.lock().iter().all(|c| c.c.complete())
    }

    fn has_unowned_work(&self) -> bool {
        !self.size_known.load(Ordering::SeqCst) || self.chunks.lock().iter().any(|c| c.owner.is_none() && !c.c.complete())
    }

    fn any_owned(&self) -> bool {
        self.chunks.lock().iter().any(|c| c.owner.is_some())
    }

    /// Claim a chunk for source `src`: the start of the largest unowned run, or its
    /// middle when another source is streaming straight into that run.
    fn claim(&self, src: usize) -> Option<usize> {
        let mut chunks = self.chunks.lock();
        let free = |c: &ChunkRt| c.owner.is_none() && !c.c.complete();
        let mut best: Option<(usize, usize, u64)> = None; // (start, len, bytes)
        let mut i = 0;
        while i < chunks.len() {
            if !free(&chunks[i]) {
                i += 1;
                continue;
            }
            let start = i;
            let mut bytes = 0;
            while i < chunks.len() && free(&chunks[i]) {
                bytes += chunks[i].c.end - chunks[i].c.pos();
                i += 1;
            }
            if best.is_none_or(|b| bytes > b.2) {
                best = Some((start, i - start, bytes));
            }
        }
        let (start, len, _) = best?;
        let fed = start > 0 && chunks[start - 1].owner.is_some();
        let idx = if fed && len > 1 { start + len / 2 } else { start };
        chunks[idx].owner = Some(src);
        Some(idx)
    }

    /// Continue into the chunk after `idx` if it is free.
    fn claim_next(&self, src: usize, idx: usize) -> bool {
        let mut chunks = self.chunks.lock();
        match chunks.get_mut(idx + 1) {
            Some(c) if c.owner.is_none() && c.c.done == 0 && !c.c.complete() => {
                c.owner = Some(src);
                true
            }
            _ => false,
        }
    }

    fn chunk(&self, idx: usize) -> Chunk {
        self.chunks.lock()[idx].c.clone()
    }

    fn advance(&self, idx: usize, n: u64) {
        let mut chunks = self.chunks.lock();
        let c = &mut chunks[idx];
        c.c.done += n;
        if c.c.complete() {
            c.owner = None;
        }
        *self.last_progress.lock() = Instant::now();
    }

    fn release(&self, src: usize) {
        for c in self.chunks.lock().iter_mut() {
            if c.owner == Some(src) {
                c.owner = None;
            }
        }
        self.changed.notify_waiters();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRef {
    pub username: String,
    pub filename: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadItem {
    pub username: String,
    pub filename: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub sources: Vec<SourceRef>,
}

#[derive(Debug)]
struct Denied(String);

impl std::fmt::Display for Denied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "denied: {}", self.0)
    }
}

impl std::error::Error for Denied {}

#[derive(Default)]
struct SourceRt {
    running: bool,
    attempts: u32,
    failed: bool,
    next_try: Option<Instant>,
}

/// Replace characters that are invalid in file names on common platforms.
pub fn sanitize_component(s: &str) -> String {
    let out: String = s.chars().map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { '_' } else { c }).collect();
    let out = out.trim().trim_end_matches('.').trim().to_owned();
    if out.is_empty() || out == ".." {
        "_".into()
    } else {
        out
    }
}

/// `dir/name.ext`, or `dir/name (n).ext` when taken.
fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    if !p.exists() {
        return p;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_owned(), format!(".{e}")),
        _ => (name.to_owned(), String::new()),
    };
    (1..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .expect("unbounded range")
}

/// A search query that finds other copies of a file.
pub fn alternate_query(filename: &str) -> String {
    let base = split_path(filename).1;
    let stem = base.rsplit_once('.').map(|(s, _)| s).unwrap_or(base);
    let words: Vec<String> = stem
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 2)
        .map(str::to_lowercase)
        .collect();
    words.into_iter().take(6).collect::<Vec<_>>().join(" ")
}

fn recompute_state(e: &mut Entry) {
    if e.t.state.is_finished() {
        return;
    }
    let s = &e.t.sources;
    e.t.state = if s.iter().any(|x| x.state == SourceState::Active) {
        TransferState::InProgress
    } else if s.iter().any(|x| x.state == SourceState::Queued) {
        TransferState::RemoteQueued
    } else {
        TransferState::Queued
    };
}

impl Client {
    pub(crate) fn part_path(&self, id: &str) -> PathBuf {
        let c = self.config.read();
        c.resolve(&self.base_dir, &c.directories.incomplete).join(format!("{id}.part"))
    }

    fn destination_root(&self, dest: Option<&str>) -> PathBuf {
        let c = self.config.read();
        let p = dest
            .and_then(|n| c.directories.destinations.iter().find(|d| d.name == n))
            .map(|d| d.path.clone())
            .unwrap_or_else(|| c.directories.downloads.clone());
        c.resolve(&self.base_dir, &p)
    }

    /// Enqueue downloads. `destination` names an entry of `directories.destinations`.
    pub fn add_downloads(self: &Arc<Self>, items: Vec<DownloadItem>, destination: Option<String>) -> Result<Vec<Transfer>> {
        let destination = destination.filter(|d| !d.is_empty());
        if let Some(d) = &destination {
            if !self.config.read().directories.destinations.iter().any(|x| &x.name == d) {
                bail!("unknown destination {d:?}");
            }
        }
        let mut out = Vec::new();
        let mut start = Vec::new();
        for item in items {
            if item.username.is_empty() || item.filename.is_empty() {
                bail!("username and filename are required");
            }
            let existing = self
                .transfers
                .inner
                .lock()
                .iter()
                .find(|e| {
                    e.t.direction == Direction::Download
                        && !e.t.state.is_finished()
                        && e.t.sources.iter().any(|s| s.username == item.username && s.filename == item.filename)
                })
                .map(|e| e.t.clone());
            if let Some(t) = existing {
                out.push(t);
                continue;
            }
            let mut sources = vec![TransferSource {
                username: item.username.clone(),
                filename: item.filename.clone(),
                state: SourceState::Idle,
                bytes: 0,
            }];
            for s in item.sources {
                if s.username.is_empty() || self.is_blacklisted(&s.username) || sources.iter().any(|x| x.username == s.username) {
                    continue;
                }
                sources.push(TransferSource { username: s.username, filename: s.filename, state: SourceState::Idle, bytes: 0 });
            }
            for s in &sources {
                self.watch_user(&s.username);
            }
            let t = Transfer {
                id: uuid::Uuid::new_v4().to_string(),
                direction: Direction::Download,
                username: item.username,
                filename: item.filename,
                size: item.size,
                state: TransferState::Queued,
                state_detail: None,
                bytes_transferred: 0,
                average_speed: 0.0,
                place_in_queue: None,
                requested_at: chrono::Utc::now(),
                started_at: None,
                ended_at: None,
                local_path: None,
                sources,
            };
            self.transfers.inner.lock().insert(Entry {
                t: t.clone(),
                destination: destination.clone(),
                dirty: false,
                task: None,
                job: None,
                resume: vec![],
                started: None,
                start_bytes: 0,
            });
            self.persist_transfer(&t.id);
            self.emit(Event::TransferUpdated(Box::new(t.clone())));
            start.push(t.id.clone());
            out.push(t);
        }
        if self.is_logged_in() {
            for id in start {
                self.start_download(&id);
            }
        }
        Ok(out)
    }

    pub fn retry_download(self: &Arc<Self>, id: &str) -> Result<Transfer> {
        {
            let mut inner = self.transfers.inner.lock();
            let e = inner.map.get_mut(id).ok_or_else(|| anyhow!("no such transfer"))?;
            if e.t.direction != Direction::Download {
                bail!("not a download");
            }
            if !e.t.state.is_finished() {
                return Ok(e.t.clone());
            }
            if e.t.state == TransferState::Succeeded {
                // download again from scratch
                e.resume.clear();
                e.t.bytes_transferred = 0;
                e.t.local_path = None;
            }
            e.t.state = TransferState::Queued;
            e.t.state_detail = None;
            e.t.ended_at = None;
            for s in &mut e.t.sources {
                s.state = SourceState::Idle;
            }
        }
        self.persist_transfer(id);
        let t = self.transfer(id).ok_or_else(|| anyhow!("no such transfer"))?;
        self.emit(Event::TransferUpdated(Box::new(t.clone())));
        if self.is_logged_in() {
            self.start_download(id);
        }
        Ok(t)
    }

    /// Restart every unfinished download that has no running task (after login).
    pub(crate) fn resume_downloads(self: &Arc<Self>) {
        let ids: Vec<String> = self
            .transfers
            .inner
            .lock()
            .iter()
            .filter(|e| e.t.direction == Direction::Download && !e.t.state.is_finished() && e.task.is_none())
            .map(|e| e.t.id.clone())
            .collect();
        for id in ids {
            self.start_download(&id);
        }
    }

    fn start_download(self: &Arc<Self>, id: &str) {
        let mut inner = self.transfers.inner.lock();
        let Some(e) = inner.map.get_mut(id) else { return };
        if e.task.as_ref().is_some_and(|t| !t.is_finished()) {
            return;
        }
        let me = self.clone();
        let id2 = id.to_owned();
        let h = tokio::spawn(async move { me.run_download(id2).await });
        e.task = Some(h.abort_handle());
    }

    async fn run_download(self: Arc<Self>, id: String) {
        let result = self.download_inner(&id).await;
        if let Err(e) = result {
            let msg = format!("{e:#}");
            tracing::info!("download {id} failed: {msg}");
            self.counters.downloads_failed.fetch_add(1, Ordering::Relaxed);
            self.update_transfer(&id, true, |e| {
                e.t.state = TransferState::Failed;
                e.t.state_detail = Some(msg);
                e.t.place_in_queue = None;
                for s in &mut e.t.sources {
                    if s.state != SourceState::Failed {
                        s.state = SourceState::Idle;
                    }
                }
            });
        }
        let mut inner = self.transfers.inner.lock();
        if let Some(e) = inner.map.get_mut(&id) {
            e.task = None;
            if let Some(j) = e.job.take() {
                if !e.t.state.is_finished() || e.t.state != TransferState::Succeeded {
                    e.resume = j.chunks_snapshot();
                }
            }
        }
    }

    fn set_source_state(&self, id: &str, i: usize, st: SourceState) {
        self.update_transfer(id, true, |e| {
            if let Some(s) = e.t.sources.get_mut(i) {
                s.state = st;
            }
            if st == SourceState::Active {
                e.t.place_in_queue = None;
            }
            recompute_state(e);
        });
    }

    async fn download_inner(self: &Arc<Self>, id: &str) -> Result<()> {
        let (size, resume, destination) = {
            let inner = self.transfers.inner.lock();
            let e = inner.map.get(id).ok_or_else(|| anyhow!("transfer removed"))?;
            (e.t.size, e.resume.clone(), e.destination.clone())
        };
        let cfg = self.config();
        let chunk_size = if cfg.transfers.multi_source { cfg.transfers.multi_source_chunk_mb.max(1) * 1024 * 1024 } else { u64::MAX };
        let part = self.part_path(id);
        if let Some(dir) = part.parent() {
            tokio::fs::create_dir_all(dir).await?;
        }
        let job = Arc::new(DownloadJob::new(id, size, part.clone(), chunk_size, resume));
        let existing = tokio::fs::metadata(&part).await.ok().map(|m| m.len());
        if existing != Some(size) {
            job.reset_progress();
            let f = tokio::fs::File::create(&part).await?;
            f.set_len(size).await?;
        }
        {
            let mut inner = self.transfers.inner.lock();
            let e = inner.map.get_mut(id).ok_or_else(|| anyhow!("transfer removed"))?;
            e.job = Some(job.clone());
            e.resume.clear();
            e.t.bytes_transferred = job.done_bytes();
        }

        let stalled = Duration::from_secs(cfg.transfers.stalled_timeout_seconds.max(10));
        let auto_replace = cfg.transfers.auto_replace_stalled;
        let mut set: JoinSet<Result<()>> = JoinSet::new();
        let mut task_src: HashMap<tokio::task::Id, usize> = HashMap::new();
        let mut rt: Vec<SourceRt> = Vec::new();
        let mut last_replace: Option<Instant> = None;
        let mut last_error = String::from("all sources failed");
        let mut extra = 0usize;
        *job.last_progress.lock() = Instant::now();

        loop {
            if job.is_complete() {
                break;
            }
            if job.cancelled.load(Ordering::SeqCst) {
                bail!("cancelled");
            }
            let sources: Vec<TransferSource> =
                self.transfer(id).map(|t| t.sources).ok_or_else(|| anyhow!("transfer removed"))?;
            while rt.len() < sources.len() {
                let failed = sources[rt.len()].state == SourceState::Failed;
                rt.push(SourceRt { failed, ..Default::default() });
            }
            let now = Instant::now();
            let base_cap = if job.chunk_count() > 1 { MAX_PARALLEL_SOURCES } else { 1 };
            let cap = base_cap + extra;
            let mut running = rt.iter().filter(|r| r.running).count();
            for (i, s) in sources.iter().enumerate() {
                if running >= cap || !job.has_unowned_work() {
                    break;
                }
                let r = &mut rt[i];
                if r.running || r.failed || r.next_try.is_some_and(|t| t > now) {
                    continue;
                }
                if self.is_blacklisted(&s.username) || self.guard.is_banned(&s.username) {
                    r.failed = true;
                    self.set_source_state(id, i, SourceState::Failed);
                    continue;
                }
                r.running = true;
                running += 1;
                let (me, job2, u, f) = (self.clone(), job.clone(), s.username.clone(), s.filename.clone());
                let h = set.spawn(async move {
                    let r = me.source_session(&job2, i, &u, &f).await;
                    job2.release(i);
                    r
                });
                task_src.insert(h.id(), i);
            }

            if rt.iter().all(|r| r.failed) {
                if auto_replace && last_replace.is_none_or(|t| t.elapsed() > REPLACE_INTERVAL) {
                    last_replace = Some(Instant::now());
                    if self.find_alternates(id).await > 0 {
                        continue;
                    }
                }
                bail!("{last_error}");
            }

            // Stalled: nothing streaming and no progress for a while → look for more sources.
            if auto_replace
                && !job.any_owned()
                && job.last_progress.lock().elapsed() > stalled
                && last_replace.is_none_or(|t| t.elapsed() > REPLACE_INTERVAL)
            {
                last_replace = Some(Instant::now());
                let added = self.find_alternates(id).await;
                if added > 0 {
                    extra = (extra + added).min(3);
                    continue;
                }
            }

            tokio::select! {
                Some(res) = set.join_next_with_id(), if !set.is_empty() => {
                    let (tid, r) = match res {
                        Ok((tid, r)) => (tid, r),
                        Err(e) => (e.id(), Err(anyhow!("source task ended: {e}"))),
                    };
                    let Some(i) = task_src.remove(&tid) else { continue };
                    job.release(i);
                    let src = &mut rt[i];
                    src.running = false;
                    match r {
                        Ok(()) => {
                            src.attempts = 0;
                            self.set_source_state(id, i, SourceState::Idle);
                        }
                        Err(e) => {
                            let user = sources.get(i).map(|s| s.username.clone()).unwrap_or_default();
                            last_error = format!("{user}: {e:#}");
                            tracing::debug!("download {id} source {user}: {e:#}");
                            src.attempts += 1;
                            let permanent = e
                                .downcast_ref::<Denied>()
                                .is_some_and(|d| PERMANENT_DENIALS.iter().any(|p| d.0.eq_ignore_ascii_case(p)));
                            if permanent || src.attempts >= MAX_SOURCE_ATTEMPTS {
                                src.failed = true;
                                self.set_source_state(id, i, SourceState::Failed);
                                self.record_reputation(&user, |r| r.downloads_failed += 1);
                            } else {
                                src.next_try = Some(Instant::now() + Duration::from_secs(15 << src.attempts));
                                self.set_source_state(id, i, SourceState::Idle);
                            }
                            self.update_transfer(id, true, |e| e.t.state_detail = Some(last_error.clone()));
                        }
                    }
                }
                _ = job.changed.notified() => {}
                _ = tokio::time::sleep(Duration::from_secs(2)) => {}
            }
        }

        set.abort_all();
        self.finalize_download(id, &job, destination.as_deref()).await
    }

    async fn finalize_download(self: &Arc<Self>, id: &str, job: &DownloadJob, destination: Option<&str>) -> Result<()> {
        let t = self.transfer(id).ok_or_else(|| anyhow!("transfer removed"))?;
        let (dir, base) = split_path(&t.filename);
        let folder = split_path(dir).1;
        let mut target_dir = self.destination_root(destination);
        if !folder.is_empty() {
            target_dir = target_dir.join(sanitize_component(folder));
        }
        tokio::fs::create_dir_all(&target_dir).await?;
        let target = unique_path(&target_dir, &sanitize_component(base));
        if tokio::fs::rename(&job.part_path, &target).await.is_err() {
            // different filesystem
            tokio::fs::copy(&job.part_path, &target).await?;
            let _ = tokio::fs::remove_file(&job.part_path).await;
        }
        tracing::info!("downloaded {} from {}", target.display(), t.username);
        self.counters.downloads_succeeded.fetch_add(1, Ordering::Relaxed);
        let t = self
            .update_transfer(id, true, |e| {
                e.t.state = TransferState::Succeeded;
                e.t.state_detail = None;
                e.t.bytes_transferred = e.t.size;
                e.t.place_in_queue = None;
                e.t.local_path = Some(target.to_string_lossy().into_owned());
                for s in &mut e.t.sources {
                    if s.state != SourceState::Failed {
                        s.state = SourceState::Idle;
                    }
                }
                e.job = None;
                e.resume.clear();
            })
            .ok_or_else(|| anyhow!("transfer removed"))?;
        for s in t.sources.iter().filter(|s| s.bytes > 0) {
            let bytes = s.bytes;
            self.record_reputation(&s.username, |r| {
                r.downloads_succeeded += 1;
                r.bytes_downloaded += bytes;
            });
        }
        self.on_download_complete(&t);
        Ok(())
    }

    /// One attempt at fetching data from one source.
    async fn source_session(self: &Arc<Self>, job: &DownloadJob, i: usize, username: &str, filename: &str) -> Result<()> {
        let id = &job.id;
        if self.is_blacklisted(username) {
            return Err(Denied("Banned".into()).into());
        }
        let key = format!("dl:{username}:{filename}");
        let mut rx = self.register(key.clone());
        self.set_source_state(id, i, SourceState::Queued);
        self.peer_send(username, PeerMessage::QueueUpload { filename: filename.to_owned() }).await?;
        let mut requeues = 0;
        let mut last_place = Instant::now();
        let (token, rsize) = loop {
            match tokio::time::timeout(Duration::from_secs(30), &mut rx).await {
                Ok(Ok(Waited::Peer(PeerMessage::TransferRequest { token, size, .. }))) => break (token, size),
                Ok(Ok(Waited::Peer(PeerMessage::UploadDenied { reason, .. }))) => return Err(Denied(reason).into()),
                Ok(Ok(Waited::Peer(PeerMessage::UploadFailed { .. }))) => {
                    // often a leftover from an earlier session; ask again a few times
                    requeues += 1;
                    if requeues > 3 {
                        bail!("upload failed on the remote side");
                    }
                    rx = self.register(key.clone());
                    self.peer_send(username, PeerMessage::QueueUpload { filename: filename.to_owned() }).await?;
                }
                Ok(Ok(_)) => bail!("unexpected reply"),
                Ok(Err(_)) => bail!("cancelled"),
                Err(_) => {
                    if self.user_status.lock().get(username).is_some_and(|s| s.0 == UserStatus::Offline) {
                        bail!("{username} went offline");
                    }
                    if last_place.elapsed() >= PLACE_REFRESH {
                        last_place = Instant::now();
                        let _ = self.peer_send(username, PeerMessage::PlaceInQueueRequest { filename: filename.to_owned() }).await;
                    }
                }
            }
        };
        if let Some(s) = rsize {
            if job.learn_size(s) {
                self.update_transfer(id, true, |e| e.t.size = s);
            } else if s != job.size() {
                let _ = self
                    .peer_send(username, PeerMessage::TransferResponse { token, allowed: false, size: None, reason: Some("Cancelled".into()) })
                    .await;
                return Err(Denied(format!("size mismatch ({s} != {})", job.size())).into());
            }
        } else if job.size() == 0 && !job.size_known.load(Ordering::SeqCst) {
            bail!("{username} did not report the file size");
        }
        let (ftx, frx) = oneshot::channel();
        self.file_waiters.lock().insert((username.to_owned(), token), ftx);
        self.peer_send(username, PeerMessage::TransferResponse { token, allowed: true, size: None, reason: None }).await?;
        let mut stream = match tokio::time::timeout(Duration::from_secs(60), frx).await {
            Ok(Ok(s)) => s,
            _ => {
                self.file_waiters.lock().remove(&(username.to_owned(), token));
                bail!("{username} did not open a file connection");
            }
        };
        let Some(mut idx) = job.claim(i) else {
            // another source already covers everything; drop the connection
            return Ok(());
        };
        self.set_source_state(id, i, SourceState::Active);
        let mut pos = job.chunk(idx).pos();
        stream.write_all(&pos.to_le_bytes()).await?;
        let mut file = tokio::fs::OpenOptions::new().write(true).open(&job.part_path).await?;
        file.seek(SeekFrom::Start(pos)).await?;
        let stalled = Duration::from_secs(self.config.read().transfers.stalled_timeout_seconds.max(10));
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let c = job.chunk(idx);
            let remaining = c.end - c.pos();
            if remaining == 0 {
                if job.claim_next(i, idx) {
                    idx += 1;
                    continue;
                }
                break;
            }
            let want = (remaining as usize).min(buf.len());
            self.download_limiter.acquire(want).await;
            let n = match tokio::time::timeout(stalled, stream.read(&mut buf[..want])).await {
                Ok(Ok(0)) => bail!("connection closed at byte {pos}"),
                Ok(Ok(n)) => n,
                Ok(Err(e)) => bail!("read error: {e}"),
                Err(_) => bail!("stalled for {}s", stalled.as_secs()),
            };
            file.write_all(&buf[..n]).await?;
            pos += n as u64;
            job.advance(idx, n as u64);
            self.download_meter.add(n as u64);
            let total = job.done_bytes();
            self.update_transfer(id, false, |e| {
                e.t.bytes_transferred = total;
                if let Some(s) = e.t.sources.get_mut(i) {
                    s.bytes += n as u64;
                }
            });
        }
        file.flush().await?;
        Ok(())
    }

    /// Search the network for other copies (same name and size) and add them as sources.
    async fn find_alternates(self: &Arc<Self>, id: &str) -> usize {
        let Some(t) = self.transfer(id) else { return 0 };
        let query = alternate_query(&t.filename);
        if query.is_empty() || t.size == 0 || !self.is_logged_in() {
            return 0;
        }
        tracing::info!("searching alternates for {} ({query})", split_path(&t.filename).1);
        let search = match self.start_search(&query) {
            Ok(s) => s,
            Err(_) => return 0,
        };
        let responses = self.wait_search(&search.id).await;
        self.delete_search(&search.id);
        let base = split_path(&t.filename).1.to_lowercase();
        let known: HashSet<String> = t.sources.iter().map(|s| s.username.clone()).collect();
        let mut found = Vec::new();
        for r in responses {
            if known.contains(&r.username) || self.is_blacklisted(&r.username) || found.len() >= MAX_ALTERNATES_PER_SEARCH {
                continue;
            }
            if let Some(f) = r.files.iter().find(|f| f.size == t.size && split_path(&f.filename).1.to_lowercase() == base) {
                found.push(TransferSource { username: r.username.clone(), filename: f.filename.clone(), state: SourceState::Idle, bytes: 0 });
            }
        }
        let n = found.len();
        if n > 0 {
            for s in &found {
                self.watch_user(&s.username);
            }
            self.update_transfer(id, true, |e| {
                e.t.sources.extend(found);
                e.t.state_detail = Some(format!("Added {n} alternate source(s)"));
            });
        }
        n
    }

    /// Ask the primary queued source for our place in its queue.
    pub async fn refresh_place_in_queue(self: &Arc<Self>, id: &str) -> Result<Transfer> {
        let t = self.transfer(id).ok_or_else(|| anyhow!("no such transfer"))?;
        let src = t
            .sources
            .iter()
            .find(|s| s.state == SourceState::Queued)
            .or(t.sources.first())
            .cloned()
            .ok_or_else(|| anyhow!("no source"))?;
        let key = format!("place:{}:{}", src.username, src.filename);
        let _ = self
            .peer_request(&src.username, PeerMessage::PlaceInQueueRequest { filename: src.filename.clone() }, key, Duration::from_secs(5))
            .await;
        self.transfer(id).ok_or_else(|| anyhow!("no such transfer"))
    }

    pub(crate) fn on_place_in_queue(&self, username: &str, filename: &str, place: u32) {
        let ids: Vec<String> = self
            .transfers
            .inner
            .lock()
            .iter()
            .filter(|e| {
                e.t.direction == Direction::Download
                    && !e.t.state.is_finished()
                    && e.t.sources.iter().any(|s| s.username == username && s.filename == filename)
            })
            .map(|e| e.t.id.clone())
            .collect();
        for id in ids {
            self.update_transfer(&id, true, |e| e.t.place_in_queue = Some(place));
        }
        self.complete(&format!("place:{username}:{filename}"), || {
            Waited::Peer(PeerMessage::PlaceInQueueResponse { filename: filename.to_owned(), place })
        });
    }

    pub(crate) fn on_remote_upload_failed(&self, username: &str, filename: &str, reason: Option<String>) {
        let msg = match reason {
            Some(reason) => PeerMessage::UploadDenied { filename: filename.to_owned(), reason },
            None => PeerMessage::UploadFailed { filename: filename.to_owned() },
        };
        self.complete(&format!("dl:{username}:{filename}"), || Waited::Peer(msg.clone()));
    }

    /// A peer's TransferRequest: either an upload to us (answer to our QueueUpload)
    /// or a legacy download request.
    pub(crate) fn on_transfer_request(self: &Arc<Self>, username: &str, direction: u32, token: u32, filename: String, size: Option<u64>) {
        let me = self.clone();
        let u = username.to_owned();
        if direction == DIRECTION_UPLOAD {
            let msg = PeerMessage::TransferRequest { direction, token, filename: filename.clone(), size };
            if !self.complete(&format!("dl:{u}:{filename}"), || Waited::Peer(msg.clone())) {
                tokio::spawn(async move {
                    let _ = me
                        .peer_send(&u, PeerMessage::TransferResponse { token, allowed: false, size: None, reason: Some("Cancelled".into()) })
                        .await;
                });
            }
        } else {
            tokio::spawn(async move {
                let reason = match me.enqueue_upload(&u, &filename) {
                    Ok(()) => "Queued".to_owned(),
                    Err(r) => r,
                };
                let _ = me.peer_send(&u, PeerMessage::TransferResponse { token, allowed: false, size: None, reason: Some(reason) }).await;
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_layout_and_claims() {
        let mb = 1024 * 1024;
        let job = DownloadJob::new("x", 10 * mb, PathBuf::from("/tmp/x"), 2 * mb, vec![]);
        assert_eq!(job.chunk_count(), 5);
        assert_eq!(job.claim(0), Some(0));
        // second source starts in the middle of the run that source 0 streams into
        assert_eq!(job.claim(1), Some(3));
        job.advance(0, 2 * mb);
        assert!(job.claim_next(0, 0));
        assert_eq!(job.chunk(1).pos(), 2 * mb);
        job.release(1);
        assert!(job.has_unowned_work());
        let snap = job.chunks_snapshot();
        assert!(valid_layout(&snap, 10 * mb));
        let resumed = DownloadJob::new("x", 10 * mb, PathBuf::from("/tmp/x"), 4 * mb, snap.clone());
        assert_eq!(resumed.chunks_snapshot(), snap);
        assert_eq!(resumed.done_bytes(), 2 * mb);
    }

    #[test]
    fn unknown_size_is_learned() {
        let job = DownloadJob::new("x", 0, PathBuf::from("/tmp/x"), 1, vec![]);
        assert!(!job.is_complete());
        assert!(job.has_unowned_work());
        assert!(job.learn_size(5 * 1024 * 1024));
        assert_eq!(job.chunk_count(), 5);
        assert!(!job.learn_size(1));
        let empty = DownloadJob::new("x", 0, PathBuf::from("/tmp/x"), 1, vec![]);
        empty.learn_size(0);
        assert!(empty.is_complete());
    }

    #[test]
    fn helpers() {
        assert_eq!(sanitize_component("a/b:c?"), "a_b_c_");
        assert_eq!(sanitize_component(".."), "_");
        assert_eq!(alternate_query("@@x\\Music\\Artist - Song (Live).flac"), "artist song live");
    }
}
