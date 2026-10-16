//! Local files: a small file manager confined to configured roots (downloads,
//! incomplete, destinations, shared folders), a host directory picker for
//! choosing new shares, and HTTP Range streaming for the web player.

use crate::api::ApiError;
use axum::body::Body;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use serde::Serialize;
use std::path::{Component, Path, PathBuf};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    name: String,
    shared: bool,
    path: String,
    is_directory: bool,
    size: u64,
    modified_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Resolve `rel` inside `root`, refusing anything that escapes it (including via symlinks).
pub fn confine(root: &Path, rel: &str) -> Result<PathBuf, ApiError> {
    let rel = rel.trim_start_matches(['/', '\\']);
    let rel_path = Path::new(rel);
    if rel_path.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir)) {
        return Err(ApiError::bad_request("invalid path"));
    }
    let root = root.canonicalize().map_err(|_| ApiError::not_found("folder does not exist"))?;
    let full = root.join(rel_path);
    let canon = full.canonicalize().map_err(|_| ApiError::not_found("no such file"))?;
    if !canon.starts_with(&root) {
        return Err(ApiError::bad_request("path is outside the folder"));
    }
    Ok(canon)
}

/// Canonical share roots, for marking entries as shared.
pub fn canonical_all(paths: &[PathBuf]) -> Vec<PathBuf> {
    paths.iter().filter_map(|p| p.canonicalize().ok()).collect()
}

pub fn is_shared(p: &Path, shares: &[PathBuf]) -> bool {
    shares.iter().any(|s| p.starts_with(s))
}

pub async fn list(root: &Path, rel: &str, shares: &[PathBuf]) -> Result<Vec<FileEntry>, ApiError> {
    let dir = confine(root, rel)?;
    let root = root.canonicalize().map_err(|_| ApiError::not_found("folder does not exist"))?;
    let mut rd = tokio::fs::read_dir(&dir).await.map_err(|e| ApiError::bad_request(&format!("cannot list: {e}")))?;
    let mut out = Vec::new();
    while let Ok(Some(e)) = rd.next_entry().await {
        let Ok(md) = tokio::fs::metadata(e.path()).await else { continue };
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = e.path();
        let relp = path.strip_prefix(&root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
        out.push(FileEntry {
            name,
            shared: is_shared(&path.canonicalize().unwrap_or_else(|_| path.clone()), shares),
            path: relp,
            is_directory: md.is_dir(),
            size: if md.is_dir() { 0 } else { md.len() },
            modified_at: md.modified().ok().map(chrono::DateTime::<chrono::Utc>::from),
        });
    }
    out.sort_by(|a, b| b.is_directory.cmp(&a.is_directory).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(out)
}

/// A single, plain file or folder name: no separators, no `.`/`..`.
pub fn valid_name(name: &str) -> bool {
    let n = name.trim();
    !n.is_empty() && n != "." && n != ".." && !n.contains(['/', '\\', '\0']) && n.len() <= 255
}

/// Resolve a path that must *not* exist yet: its parent is confined to `root`
/// and the last segment is a plain name.
pub fn confine_new(root: &Path, rel: &str) -> Result<PathBuf, ApiError> {
    let rel = rel.trim_matches(['/', '\\']);
    let (parent, name) = match rel.rsplit_once('/') {
        Some((p, n)) => (p, n),
        None => ("", rel),
    };
    if !valid_name(name) {
        return Err(ApiError::bad_request("invalid name"));
    }
    let dir = confine(root, parent)?;
    if !dir.is_dir() {
        return Err(ApiError::bad_request("parent is not a folder"));
    }
    let full = dir.join(name.trim());
    if full.symlink_metadata().is_ok() {
        return Err(ApiError::bad_request("a file or folder with that name already exists"));
    }
    Ok(full)
}

pub async fn mkdir(root: &Path, rel: &str) -> Result<(), ApiError> {
    let p = confine_new(root, rel)?;
    tokio::fs::create_dir(&p).await.map_err(|e| ApiError::bad_request(&format!("cannot create folder: {e}")))?;
    Ok(())
}

pub async fn rename(root: &Path, from: &str, to: &str) -> Result<(), ApiError> {
    let src = confine(root, from)?;
    let canon_root = root.canonicalize().map_err(|_| ApiError::not_found("folder does not exist"))?;
    if src == canon_root {
        return Err(ApiError::bad_request("cannot move the root folder"));
    }
    let dst = confine_new(root, to)?;
    if dst.starts_with(&src) {
        return Err(ApiError::bad_request("cannot move a folder into itself"));
    }
    tokio::fs::rename(&src, &dst).await.map_err(|e| ApiError::bad_request(&format!("move failed: {e}")))?;
    tracing::info!("moved {} -> {}", src.display(), dst.display());
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostDir {
    name: String,
    path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostListing {
    path: String,
    parent: Option<String>,
    dirs: Vec<HostDir>,
}

/// Directories (only) at `rel` inside the storage folder, for picking
/// folders to share. Paths are relative to storage; "" is storage itself.
/// `hide` (incomplete downloads) is left out, since it cannot be shared.
pub async fn storage_dirs(storage: &Path, rel: &str, hide: Option<&Path>) -> Result<HostListing, ApiError> {
    let hide = hide.and_then(|h| h.canonicalize().ok());
    let root = storage.canonicalize().map_err(|_| ApiError::not_found("storage folder does not exist"))?;
    let dir = confine(&root, rel)?;
    let rel_of = |p: &Path| p.strip_prefix(&root).map(|r| r.display().to_string()).unwrap_or_default();
    let mut rd = tokio::fs::read_dir(&dir).await.map_err(|e| ApiError::bad_request(&format!("cannot list: {e}")))?;
    let mut dirs = Vec::new();
    while let Ok(Some(e)) = rd.next_entry().await {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        // Symlinks are skipped: they could lead out of storage.
        if hide.as_deref() == Some(e.path().as_path()) {
            continue;
        }
        if tokio::fs::symlink_metadata(e.path()).await.map(|m| m.is_dir()).unwrap_or(false) {
            dirs.push(HostDir { path: rel_of(&e.path()), name });
        }
    }
    dirs.sort_by_key(|d| d.name.to_lowercase());
    let parent = if dir == root { None } else { dir.parent().map(rel_of) };
    Ok(HostListing { path: rel_of(&dir), parent, dirs })
}

pub async fn delete(root: &Path, rel: &str) -> Result<(), ApiError> {
    let p = confine(root, rel)?;
    let root = root.canonicalize().map_err(|_| ApiError::not_found("folder does not exist"))?;
    if p == root {
        return Err(ApiError::bad_request("refusing to delete a root folder"));
    }
    let md = tokio::fs::metadata(&p).await.map_err(|_| ApiError::not_found("no such file"))?;
    let r = if md.is_dir() { tokio::fs::remove_dir_all(&p).await } else { tokio::fs::remove_file(&p).await };
    r.map_err(|e| ApiError::bad_request(&format!("delete failed: {e}")))?;
    tracing::info!("deleted {}", p.display());
    Ok(())
}

/// Parse a single `bytes=` range against a file of `len` bytes.
fn parse_range(v: &str, len: u64) -> Option<(u64, u64)> {
    let spec = v.trim().strip_prefix("bytes=")?;
    if spec.contains(',') || len == 0 {
        return None;
    }
    let (a, b) = spec.split_once('-')?;
    let (start, end) = match (a.trim(), b.trim()) {
        ("", suffix) => {
            let n: u64 = suffix.parse().ok()?;
            (len.saturating_sub(n), len - 1)
        }
        (s, "") => (s.parse().ok()?, len - 1),
        (s, e) => (s.parse().ok()?, e.parse::<u64>().ok()?.min(len - 1)),
    };
    (start <= end && start < len).then_some((start, end))
}

pub async fn stream(path: &Path, headers: &HeaderMap, attachment: bool) -> Result<Response, ApiError> {
    let mut f = tokio::fs::File::open(path).await.map_err(|_| ApiError::not_found("file not found"))?;
    let md = f.metadata().await.map_err(|_| ApiError::not_found("file not found"))?;
    if !md.is_file() {
        return Err(ApiError::bad_request("not a file"));
    }
    let len = md.len();
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let range = headers.get(header::RANGE).and_then(|v| v.to_str().ok());
    let mut resp = Response::builder()
        .header(header::CONTENT_TYPE, mime.as_ref())
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CACHE_CONTROL, "private, max-age=3600");
    let (status, start, count) = match range {
        Some(r) => match parse_range(r, len) {
            Some((s, e)) => {
                resp = resp.header(header::CONTENT_RANGE, format!("bytes {s}-{e}/{len}"));
                (StatusCode::PARTIAL_CONTENT, s, e - s + 1)
            }
            None => {
                return Ok(Response::builder()
                    .status(StatusCode::RANGE_NOT_SATISFIABLE)
                    .header(header::CONTENT_RANGE, format!("bytes */{len}"))
                    .body(Body::empty())
                    .unwrap());
            }
        },
        None => (StatusCode::OK, 0, len),
    };
    if start > 0 {
        f.seek(std::io::SeekFrom::Start(start)).await.map_err(|e| ApiError::internal(&e.to_string()))?;
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let disp = format!("{}; filename*=UTF-8''{}", if attachment { "attachment" } else { "inline" }, encode_rfc5987(&name));
    let resp = resp
        .status(status)
        .header(header::CONTENT_LENGTH, count)
        .header(header::CONTENT_DISPOSITION, HeaderValue::from_str(&disp).unwrap_or(HeaderValue::from_static("inline")))
        .body(Body::from_stream(ReaderStream::new(f.take(count))))
        .map_err(|e| ApiError::internal(&e.to_string()))?;
    Ok(resp)
}

fn encode_rfc5987(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        assert_eq!(parse_range("bytes=0-99", 1000), Some((0, 99)));
        assert_eq!(parse_range("bytes=900-", 1000), Some((900, 999)));
        assert_eq!(parse_range("bytes=-100", 1000), Some((900, 999)));
        assert_eq!(parse_range("bytes=0-5000", 1000), Some((0, 999)));
        assert_eq!(parse_range("bytes=1000-", 1000), None);
        assert_eq!(parse_range("bytes=0-1,5-6", 1000), None);
    }

    #[test]
    fn confinement() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("a")).unwrap();
        std::fs::write(d.path().join("a/x.mp3"), b"x").unwrap();
        assert!(confine(d.path(), "a/x.mp3").is_ok());
        assert!(confine(d.path(), "").is_ok());
        assert!(confine(d.path(), "../etc").is_err());
        assert!(confine(d.path(), "a/../../x").is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/", d.path().join("escape")).unwrap();
            assert!(confine(d.path(), "escape/etc").is_err());
        }
    }

    #[test]
    fn new_paths() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("a")).unwrap();
        std::fs::write(d.path().join("a/x.mp3"), b"x").unwrap();
        assert!(confine_new(d.path(), "a/y.mp3").is_ok());
        assert!(confine_new(d.path(), "new").is_ok());
        assert!(confine_new(d.path(), "a/x.mp3").is_err(), "exists");
        assert!(confine_new(d.path(), "a/..").is_err());
        assert!(confine_new(d.path(), "../x").is_err());
        assert!(confine_new(d.path(), "missing/x").is_err());
        assert!(!valid_name("a/b"));
        assert!(valid_name("Album (2001)"));
    }

    #[tokio::test]
    async fn mkdir_rename_list() {
        let d = tempfile::tempdir().unwrap();
        mkdir(d.path(), "music").await.unwrap();
        std::fs::write(d.path().join("music/a.flac"), b"x").unwrap();
        rename(d.path(), "music/a.flac", "b.flac").await.unwrap();
        assert!(d.path().join("b.flac").exists());
        assert!(rename(d.path(), "music", "music/inner").await.is_err());
        let shares = canonical_all(&[d.path().join("music")]);
        let l = list(d.path(), "", &shares).await.unwrap();
        assert_eq!(l.len(), 2);
        assert!(l.iter().find(|e| e.name == "music").unwrap().shared);
        assert!(!l.iter().find(|e| e.name == "b.flac").unwrap().shared);
    }
}
