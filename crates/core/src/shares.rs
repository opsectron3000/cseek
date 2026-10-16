//! Shared files: scanning, the word index used to answer searches, and
//! browse / folder-contents / user-info responses.

use crate::client::Client;
use crate::model::{ShareDirSummary, ShareSummary, Time};
use parking_lot::RwLock;
use slsk_proto::peer::UserInfo;
use slsk_proto::{attr, DirEntry, FileEntry, PeerMessage, ServerRequest};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

const AUDIO_EXTS: &[&str] = &["mp3", "flac", "ogg", "opus", "m4a", "aac", "wav", "aiff", "aif", "ape", "wv", "alac", "wma", "mpc"];

#[derive(Debug, Clone)]
pub struct SharedFile {
    /// Virtual path as seen by peers: `Alias\sub\file.ext`.
    pub vpath: String,
    pub local: PathBuf,
    pub size: u64,
    pub ext: String,
    pub attrs: Vec<(u32, u32)>,
}

impl SharedFile {
    fn entry(&self, full_path: bool) -> FileEntry {
        let filename = if full_path { self.vpath.clone() } else { crate::model::split_path(&self.vpath).1.to_owned() };
        FileEntry { filename, size: self.size, extension: self.ext.clone(), attributes: self.attrs.clone() }
    }
}

#[derive(Default)]
pub struct ShareIndex {
    pub files: Vec<SharedFile>,
    by_vpath: HashMap<String, usize>,
    /// Directory (virtual, lowercase-insensitive key preserved as-is) → file indices.
    dirs: BTreeMap<String, Vec<usize>>,
    words: HashMap<String, Vec<u32>>,
    roots: Vec<ShareDirSummary>,
    pub generation: u64,
}

static GENERATION: AtomicU64 = AtomicU64::new(1);

fn words_of(s: &str) -> impl Iterator<Item = String> + '_ {
    s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(|w| w.to_lowercase())
}

impl ShareIndex {
    pub fn build(files: Vec<SharedFile>, roots: Vec<ShareDirSummary>) -> Self {
        let mut ix = ShareIndex { roots, generation: GENERATION.fetch_add(1, Ordering::Relaxed), ..Default::default() };
        for (i, f) in files.iter().enumerate() {
            ix.by_vpath.insert(f.vpath.to_lowercase(), i);
            let (dir, _) = crate::model::split_path(&f.vpath);
            ix.dirs.entry(dir.to_owned()).or_default().push(i);
            let mut seen = HashSet::new();
            for w in words_of(&f.vpath) {
                if seen.insert(w.clone()) {
                    ix.words.entry(w).or_default().push(i as u32);
                }
            }
        }
        // make sure every ancestor directory exists so browse trees are complete
        let all: Vec<String> = ix.dirs.keys().cloned().collect();
        for d in all {
            let mut cur = d.as_str();
            while let Some(i) = cur.rfind('\\') {
                cur = &cur[..i];
                ix.dirs.entry(cur.to_owned()).or_default();
            }
        }
        ix.files = files;
        ix
    }

    pub fn get(&self, vpath: &str) -> Option<&SharedFile> {
        self.by_vpath.get(&vpath.to_lowercase()).map(|&i| &self.files[i])
    }

    /// Soulseek-style matching: every term must match a word of the path; `-term` excludes;
    /// `*suffix` matches words ending with suffix.
    pub fn search(&self, query: &str, limit: usize) -> Vec<&SharedFile> {
        let mut include: Vec<String> = Vec::new();
        let mut wildcard: Vec<String> = Vec::new();
        let mut exclude: Vec<String> = Vec::new();
        for term in query.split_whitespace() {
            if let Some(t) = term.strip_prefix('-') {
                exclude.extend(words_of(t));
            } else if let Some(t) = term.strip_prefix('*') {
                wildcard.extend(words_of(t));
            } else {
                include.extend(words_of(term));
            }
        }
        if include.is_empty() && wildcard.is_empty() {
            return vec![];
        }
        let mut candidates: Option<Vec<u32>> = None;
        let mut intersect = |list: Vec<u32>| {
            candidates = Some(match candidates.take() {
                None => list,
                Some(c) => {
                    let set: HashSet<u32> = list.into_iter().collect();
                    c.into_iter().filter(|i| set.contains(i)).collect()
                }
            });
        };
        let mut sorted_inc = include.clone();
        sorted_inc.sort_by_key(|w| self.words.get(w).map(|v| v.len()).unwrap_or(0));
        for w in &sorted_inc {
            match self.words.get(w) {
                Some(l) => intersect(l.clone()),
                None => return vec![],
            }
        }
        for suf in &wildcard {
            let mut l: Vec<u32> = self
                .words
                .iter()
                .filter(|(k, _)| k.ends_with(suf.as_str()))
                .flat_map(|(_, v)| v.iter().copied())
                .collect();
            l.sort_unstable();
            l.dedup();
            intersect(l);
        }
        let excl: HashSet<u32> = exclude.iter().filter_map(|w| self.words.get(w)).flatten().copied().collect();
        let mut out: Vec<&SharedFile> = candidates
            .unwrap_or_default()
            .into_iter()
            .filter(|i| !excl.contains(i))
            .map(|i| &self.files[i as usize])
            .collect();
        out.sort_by(|a, b| a.vpath.cmp(&b.vpath));
        out.truncate(limit);
        out
    }

    pub fn dir_entries(&self) -> Vec<DirEntry> {
        self.dirs
            .iter()
            .map(|(d, idx)| DirEntry { name: d.clone(), files: idx.iter().map(|&i| self.files[i].entry(false)).collect() })
            .collect()
    }

    /// Contents of `folder` and all of its subfolders.
    pub fn folder(&self, folder: &str) -> Vec<DirEntry> {
        let lower = folder.to_lowercase();
        let prefix = format!("{lower}\\");
        self.dirs
            .iter()
            .filter(|(d, _)| {
                let dl = d.to_lowercase();
                dl == lower || dl.starts_with(&prefix)
            })
            .map(|(d, idx)| DirEntry { name: d.clone(), files: idx.iter().map(|&i| self.files[i].entry(false)).collect() })
            .collect()
    }

    pub fn dir_count(&self) -> usize {
        self.dirs.len()
    }
}

#[derive(Default)]
pub struct Shares {
    pub index: RwLock<Arc<ShareIndex>>,
    scanning: AtomicBool,
    progress: AtomicU64, // fraction * 1e6
    last_scan: RwLock<Option<Time>>,
    /// Encoded browse response, keyed by index generation.
    browse_cache: parking_lot::Mutex<Option<(u64, Arc<Vec<u8>>)>>,
}

impl Shares {
    pub fn summary(&self) -> ShareSummary {
        let ix = self.index.read().clone();
        ShareSummary {
            directories: ix.roots.clone(),
            file_count: ix.files.len(),
            directory_count: ix.dir_count(),
            scanning: self.scanning.load(Ordering::Relaxed),
            scan_progress: self.progress.load(Ordering::Relaxed) as f64 / 1e6,
            last_scan: *self.last_scan.read(),
        }
    }

    pub fn counts(&self) -> (usize, usize) {
        let ix = self.index.read();
        (ix.dir_count(), ix.files.len())
    }

    pub fn snapshot(&self) -> Arc<ShareIndex> {
        self.index.read().clone()
    }
}

fn read_audio_attrs(path: &Path) -> Vec<(u32, u32)> {
    use lofty::file::AudioFile;
    let Ok(tagged) = lofty::read_from_path(path) else { return vec![] };
    let p = tagged.properties();
    let mut a = Vec::new();
    if let Some(br) = p.audio_bitrate().or(p.overall_bitrate()) {
        a.push((attr::BITRATE, br));
    }
    let secs = p.duration().as_secs();
    if secs > 0 {
        a.push((attr::DURATION, secs as u32));
    }
    if let Some(sr) = p.sample_rate() {
        a.push((attr::SAMPLE_RATE, sr));
    }
    if let Some(bd) = p.bit_depth() {
        a.push((attr::BIT_DEPTH, bd as u32));
    }
    a
}

impl Client {
    pub fn rescan_shares(self: &Arc<Self>) {
        if self.shares.scanning.swap(true, Ordering::SeqCst) {
            return;
        }
        self.shares.progress.store(0, Ordering::Relaxed);
        self.emit_app();
        let me = self.clone();
        tokio::spawn(async move {
            let me2 = me.clone();
            let res = tokio::task::spawn_blocking(move || me2.scan_blocking()).await;
            match res {
                Ok(ix) => {
                    tracing::info!("share scan complete: {} files in {} directories", ix.files.len(), ix.dir_count());
                    *me.shares.index.write() = Arc::new(ix);
                    *me.shares.last_scan.write() = Some(chrono::Utc::now());
                    let (dirs, files) = me.shares.counts();
                    let _ = me.server_send(ServerRequest::SharedFoldersFiles { dirs: dirs as u32, files: files as u32 });
                }
                Err(e) => tracing::error!("share scan failed: {e}"),
            }
            me.shares.scanning.store(false, Ordering::SeqCst);
            me.shares.progress.store(1_000_000, Ordering::Relaxed);
            me.emit_app();
        });
    }

    fn scan_blocking(&self) -> ShareIndex {
        let cfg = self.config();
        let roots = cfg.share_roots(&self.base_dir);
        let excludes: Vec<String> = cfg.shares.exclude_filters.iter().map(|s| s.to_lowercase()).filter(|s| !s.is_empty()).collect();
        let cache = self.db.share_cache_load();

        // first pass: enumerate
        let mut found: Vec<(usize, PathBuf, String, u64, i64)> = Vec::new();
        let mut summaries = Vec::new();
        for (ri, (alias, root)) in roots.iter().enumerate() {
            let mut files = 0usize;
            let mut dirs = 0usize;
            if !root.is_dir() {
                tracing::warn!("shared directory {} does not exist", root.display());
            }
            for entry in walkdir::WalkDir::new(root).follow_links(false).into_iter().filter_entry(|e| {
                e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.')
            }) {
                let Ok(entry) = entry else { continue };
                let lossy = entry.path().to_string_lossy().to_lowercase();
                if excludes.iter().any(|x| lossy.contains(x.as_str())) {
                    continue;
                }
                if entry.file_type().is_dir() {
                    dirs += 1;
                    continue;
                }
                if !entry.file_type().is_file() {
                    continue;
                }
                let Ok(meta) = entry.metadata() else { continue };
                let rel = entry.path().strip_prefix(root).unwrap_or(entry.path());
                let mut vpath = alias.clone();
                for c in rel.components() {
                    vpath.push('\\');
                    vpath.push_str(&c.as_os_str().to_string_lossy());
                }
                let mtime = meta.modified().ok().and_then(|m| m.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0);
                found.push((ri, entry.path().to_path_buf(), vpath, meta.len(), mtime));
                files += 1;
            }
            summaries.push(ShareDirSummary {
                path: root.display().to_string(),
                alias: alias.clone(),
                file_count: files,
                directory_count: dirs,
            });
        }

        // second pass: metadata (cached by size+mtime)
        let total = found.len().max(1);
        let mut files = Vec::with_capacity(found.len());
        let mut cache_rows = Vec::with_capacity(found.len());
        for (n, (_, local, vpath, size, mtime)) in found.into_iter().enumerate() {
            let ext = local.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            let key = local.to_string_lossy().into_owned();
            let attrs: Vec<(u32, u32)> = match cache.get(&key) {
                Some((s, m, a)) if *s == size && *m == mtime => serde_json::from_str(a).unwrap_or_default(),
                _ if AUDIO_EXTS.contains(&ext.as_str()) => read_audio_attrs(&local),
                _ => vec![],
            };
            cache_rows.push((key, size, mtime, serde_json::to_string(&attrs).unwrap()));
            files.push(SharedFile { vpath, local, size, ext, attrs });
            if n % 200 == 0 {
                self.shares.progress.store((n as f64 / total as f64 * 1e6) as u64, Ordering::Relaxed);
            }
        }
        self.db.share_cache_store(&cache_rows);
        ShareIndex::build(files, summaries)
    }

    pub(crate) async fn share_rescan_loop(self: Arc<Self>) {
        loop {
            let mins = self.config.read().shares.rescan_interval_minutes;
            tokio::time::sleep(Duration::from_secs(mins.max(5) * 60)).await;
            self.rescan_shares();
        }
    }

    /// Whether the user may see our shares at all.
    pub(crate) fn may_browse(&self, username: &str) -> bool {
        !self.is_blacklisted(username) && !self.guard.is_banned(username)
    }

    pub(crate) async fn send_share_list(self: &Arc<Self>, username: &str) {
        let frame = if self.may_browse(username) {
            let ix = self.shares.snapshot();
            let key = ix.generation;
            let cached = self.shares.browse_cache.lock().as_ref().filter(|(k, _)| *k == key).map(|(_, f)| f.clone());
            match cached {
                Some(f) => f,
                None => {
                    let ix2 = ix.clone();
                    let f = tokio::task::spawn_blocking(move || {
                        Arc::new(PeerMessage::SharedFileListResponse { dirs: ix2.dir_entries(), private_dirs: vec![] }.encode())
                    })
                    .await
                    .expect("encode share list");
                    *self.shares.browse_cache.lock() = Some((key, f.clone()));
                    f
                }
            }
        } else {
            Arc::new(PeerMessage::SharedFileListResponse { dirs: vec![], private_dirs: vec![] }.encode())
        };
        if let Ok(tx) = self.peer_tx_raw(username).await {
            let _ = tx.send((*frame).clone());
        }
    }

    async fn peer_tx_raw(self: &Arc<Self>, username: &str) -> anyhow::Result<tokio::sync::mpsc::UnboundedSender<Vec<u8>>> {
        if let Some(h) = self.peers.lock().get(username) {
            if !h.tx.is_closed() {
                return Ok(h.tx.clone());
            }
        }
        let stream = self.connect_peer(username, slsk_proto::init::CONN_PEER, None).await?;
        Ok(self.attach_peer(username.to_owned(), stream))
    }

    pub(crate) async fn send_folder_contents(self: &Arc<Self>, username: &str, token: u32, folder: String) {
        let dirs = if self.may_browse(username) { self.shares.snapshot().folder(&folder) } else { vec![] };
        let _ = self.peer_send(username, PeerMessage::FolderContentsResponse { token, folder, dirs }).await;
    }

    pub(crate) fn effective_description(&self) -> String {
        let cfg = self.config.read();
        let base = cfg.soulseek.description.clone();
        if !cfg.now_playing.enabled {
            return base;
        }
        match &*self.now_playing.lock() {
            Some(np) => cfg
                .now_playing
                .description_template
                .replace("{description}", &base)
                .replace("{artist}", &np.artist)
                .replace("{title}", &np.title)
                .replace("{album}", np.album.as_deref().unwrap_or("")),
            None => base,
        }
    }

    pub(crate) async fn send_user_info(self: &Arc<Self>, username: &str) {
        let picture = self.config.read().soulseek.picture.clone().filter(|p| !p.is_empty()).and_then(|p| std::fs::read(self.resolve_path(&p)).ok());
        let slots = self.config.read().transfers.upload_slots;
        let (active, queued) = self.upload_counts();
        let info = UserInfo {
            description: self.effective_description(),
            picture,
            total_uploads: slots,
            queue_size: queued as u32,
            slots_free: (active as u32) < slots,
        };
        let _ = self.peer_send(username, PeerMessage::UserInfoResponse(info)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(v: &str) -> SharedFile {
        SharedFile { vpath: v.into(), local: PathBuf::from("/x"), size: 1, ext: "mp3".into(), attrs: vec![] }
    }

    #[test]
    fn search_semantics() {
        let ix = ShareIndex::build(
            vec![
                f("Music\\Beatles\\Abbey Road\\01 Come Together.mp3"),
                f("Music\\Beatles\\Help\\01 Help.mp3"),
                f("Music\\Stones\\Sticky Fingers\\01 Brown Sugar.mp3"),
            ],
            vec![],
        );
        assert_eq!(ix.search("beatles", 10).len(), 2);
        assert_eq!(ix.search("beatles help", 10).len(), 1);
        assert_eq!(ix.search("beatles -help", 10).len(), 1);
        assert_eq!(ix.search("*tles", 10).len(), 2);
        assert_eq!(ix.search("nothing", 10).len(), 0);
        assert_eq!(ix.search("-beatles", 10).len(), 0);
        assert!(ix.get("music\\beatles\\help\\01 help.mp3").is_some());
        // ancestors exist
        let names: Vec<String> = ix.dir_entries().into_iter().map(|d| d.name).collect();
        assert!(names.contains(&"Music".to_string()));
        assert!(names.contains(&"Music\\Beatles".to_string()));
        assert_eq!(ix.folder("Music\\Beatles").len(), 3);
    }
}
