//! HTTP API (see docs/API.md).

use crate::auth;
use crate::logs::LogBuffer;
use crate::media;
use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{DefaultBodyLimit, Multipart, Path, Query, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use cseek_core::*;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone)]
pub struct Ctx {
    pub client: Arc<Client>,
    pub logs: LogBuffer,
}

// ---------- errors ----------

#[derive(Debug)]
pub struct ApiError(StatusCode, String);

impl ApiError {
    pub fn bad_request(m: &str) -> Self {
        Self(StatusCode::BAD_REQUEST, m.into())
    }
    pub fn not_found(m: &str) -> Self {
        Self(StatusCode::NOT_FOUND, m.into())
    }
    pub fn unauthorized() -> Self {
        Self(StatusCode::UNAUTHORIZED, "unauthorized".into())
    }
    pub fn internal(m: &str) -> Self {
        Self(StatusCode::INTERNAL_SERVER_ERROR, m.into())
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        let m = format!("{e:#}");
        let status = if m.contains("timed out") {
            StatusCode::GATEWAY_TIMEOUT
        } else if m.contains("not connected") || m.contains("not logged in") {
            StatusCode::SERVICE_UNAVAILABLE
        } else if m.starts_with("no such") {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::BAD_REQUEST
        };
        Self(status, m)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

type R<T> = Result<Json<T>, ApiError>;
type Empty = Result<StatusCode, ApiError>;

const NO_CONTENT: Empty = Ok(StatusCode::NO_CONTENT);

fn found<T>(v: Option<T>, what: &str) -> Result<T, ApiError> {
    v.ok_or_else(|| ApiError::not_found(&format!("no such {what}")))
}

// ---------- router ----------

pub fn router(ctx: Ctx) -> Router {
    let protected = Router::new()
        .route("/app", get(app))
        .route("/server", put(connect).delete(disconnect))
        .route("/server/status", put(set_status))
        // searches
        .route("/searches", get(searches).post(start_search).delete(clear_searches))
        .route("/searches/{id}", get(search).delete(delete_search))
        .route("/searches/{id}/responses", get(search_responses))
        .route("/searches/{id}/stop", put(stop_search))
        // transfers
        .route("/transfers/downloads", get(downloads).post(add_downloads).delete(clear_downloads))
        .route("/transfers/uploads", get(uploads).delete(clear_uploads))
        .route("/transfers/downloads/{id}", delete(cancel_download))
        .route("/transfers/uploads/{id}", delete(cancel_upload))
        .route("/transfers/downloads/{id}/retry", post(retry_download))
        .route("/transfers/downloads/{id}/position", post(refresh_position))
        // users
        .route("/users/countries", get(user_countries))
        .route("/users/{u}/info", get(user_info))
        .route("/users/{u}/picture", get(user_picture))
        .route("/users/{u}/status", get(user_status))
        .route("/users/{u}/stats", get(user_stats))
        .route("/users/{u}/address", get(user_address))
        .route("/users/{u}/browse", get(browse))
        .route("/users/{u}/directory", post(directory))
        .route("/users/{u}/interests", get(user_interests))
        .route("/users/{u}/profile", get(profile))
        .route("/users/{u}/note", put(set_note))
        .route("/users/{u}/group", put(set_group))
        .route("/reputation", get(reputation))
        // conversations
        .route("/conversations", get(conversations))
        .route("/conversations/{u}", get(conversation).post(send_message).delete(delete_conversation))
        .route("/conversations/{u}/read", put(mark_read))
        // rooms
        .route("/rooms/available", get(available_rooms))
        .route("/rooms/joined", get(joined_rooms).post(join_room))
        .route("/rooms/joined/{name}", get(room).delete(leave_room))
        .route("/rooms/joined/{name}/messages", post(say_room))
        // shares & files
        .route("/shares", get(shares))
        .route("/shares/scan", put(rescan))
        .route("/shares/directories", post(add_share_dir).delete(remove_share_dir))
        .route("/files", get(list_files).delete(delete_file))
        .route("/files/roots", get(list_roots))
        .route("/files/mkdir", post(make_dir))
        .route("/files/move", post(move_file))
        .route("/files/upload", post(upload_files).layer(DefaultBodyLimit::max(4 * 1024 * 1024 * 1024)))
        .route("/fs/dirs", get(host_dirs))
        .route("/stream/transfer/{id}", get(stream_transfer))
        .route("/stream/file", get(stream_file))
        // wishlist
        .route("/wishlist", get(wishlist).post(add_wishlist))
        .route("/wishlist/{id}", put(update_wishlist).delete(delete_wishlist))
        .route("/wishlist/{id}/run", post(run_wishlist))
        // discovery
        .route("/discovery/interests", get(interests).post(add_interest).delete(remove_interest))
        .route("/discovery/recommendations", get(recommendations))
        .route("/discovery/global", get(global_recommendations))
        .route("/discovery/similar-users", get(similar_users))
        .route("/discovery/items/{item}", get(item_info))
        // now playing
        .route("/nowplaying", get(now_playing).put(set_now_playing).delete(clear_now_playing))
        .route("/nowplaying/webhook/jellyfin", post(jellyfin_webhook))
        .route("/nowplaying/webhook/plex", post(plex_webhook))
        // integrations
        .route("/integrations/lidarr/sync", post(lidarr_sync))
        .route("/integrations/lidarr/status", get(lidarr_status))
        // system
        .route("/options", get(options).put(save_options))
        .route("/logs", get(logs))
        .route("/metrics", get(metrics_json))
        .route("/guard", get(guard))
        .route("/guard/bans/{u}", delete(unban))
        .route("/groups", get(groups))
        .route("/events", get(events))
        .layer(axum::middleware::from_fn_with_state(ctx.clone(), auth::require_auth));

    let api = Router::new()
        .route("/session", post(login).get(session).delete(logout))
        .merge(protected)
        .fallback(|| async { ApiError::not_found("no such endpoint") });

    Router::new()
        .nest("/api", api)
        .route(
            "/metrics",
            get(prometheus).layer(axum::middleware::from_fn_with_state(ctx.clone(), auth::require_auth)),
        )
        .with_state(ctx)
}

// ---------- session ----------

#[derive(Deserialize)]
struct LoginBody {
    username: String,
    password: String,
}

async fn login(State(ctx): State<Ctx>, Json(b): Json<LoginBody>) -> R<Value> {
    let cfg = ctx.client.config().web.auth;
    if cfg.disabled {
        let token = auth::create_session(&ctx, &b.username);
        return Ok(Json(json!({ "token": token, "username": b.username })));
    }
    let ok = auth::ct_eq(&b.username, &cfg.username) & auth::ct_eq(&b.password, &cfg.password);
    if !ok || cfg.password.is_empty() {
        tracing::warn!("failed web login for {:?}", b.username);
        // slow down guessing
        tokio::time::sleep(std::time::Duration::from_millis(800)).await;
        return Err(ApiError(StatusCode::UNAUTHORIZED, "invalid username or password".into()));
    }
    let token = auth::create_session(&ctx, &b.username);
    Ok(Json(json!({ "token": token, "username": b.username })))
}

async fn session(State(ctx): State<Ctx>, req: Request) -> R<Value> {
    if ctx.client.config().web.auth.disabled {
        return Ok(Json(json!({ "username": "anonymous", "authDisabled": true })));
    }
    let user = auth::request_credential(&req).and_then(|c| auth::check_credential(&ctx, &c)).ok_or_else(ApiError::unauthorized)?;
    Ok(Json(json!({ "username": user, "authDisabled": false })))
}

async fn logout(State(ctx): State<Ctx>, req: Request) -> Empty {
    if let Some(c) = auth::request_credential(&req) {
        auth::delete_session(&ctx, &c);
    }
    NO_CONTENT
}

// ---------- app / server ----------

async fn app(State(ctx): State<Ctx>) -> Json<AppState> {
    Json(ctx.client.app_state())
}

async fn connect(State(ctx): State<Ctx>) -> Empty {
    let c = ctx.client.config();
    if c.soulseek.username.is_empty() || c.soulseek.password.is_empty() {
        return Err(ApiError::bad_request("set a Soulseek username and password in Options first"));
    }
    ctx.client.connect();
    NO_CONTENT
}

async fn disconnect(State(ctx): State<Ctx>) -> Empty {
    ctx.client.disconnect();
    NO_CONTENT
}

#[derive(Deserialize)]
struct StatusBody {
    status: UserStatus,
}

async fn set_status(State(ctx): State<Ctx>, Json(b): Json<StatusBody>) -> Empty {
    if b.status == UserStatus::Offline {
        return Err(ApiError::bad_request("status must be online or away"));
    }
    ctx.client.set_status(b.status)?;
    NO_CONTENT
}

// ---------- searches ----------

#[derive(Deserialize)]
struct QueryBody {
    query: String,
}

async fn searches(State(ctx): State<Ctx>) -> Json<Vec<Search>> {
    Json(ctx.client.searches())
}

async fn start_search(State(ctx): State<Ctx>, Json(b): Json<QueryBody>) -> R<Search> {
    Ok(Json(ctx.client.start_search(&b.query)?))
}

async fn search(State(ctx): State<Ctx>, Path(id): Path<String>) -> R<Search> {
    Ok(Json(found(ctx.client.search(&id), "search")?))
}

async fn search_responses(State(ctx): State<Ctx>, Path(id): Path<String>) -> R<Vec<SearchResponse>> {
    Ok(Json(found(ctx.client.search_responses(&id), "search")?))
}

async fn stop_search(State(ctx): State<Ctx>, Path(id): Path<String>) -> R<Search> {
    Ok(Json(found(ctx.client.stop_search(&id), "search")?))
}

async fn delete_search(State(ctx): State<Ctx>, Path(id): Path<String>) -> Empty {
    ctx.client.delete_search(&id);
    NO_CONTENT
}

async fn clear_searches(State(ctx): State<Ctx>) -> Empty {
    ctx.client.clear_searches();
    NO_CONTENT
}

// ---------- transfers ----------

#[derive(Deserialize)]
struct DownloadBody {
    items: Vec<DownloadItem>,
    destination: Option<String>,
}

#[derive(Deserialize)]
struct RemoveQuery {
    remove: Option<bool>,
}

#[derive(Deserialize)]
struct ClearQuery {
    state: Option<String>,
}

/// A transfer as clients see it: without the host path of its local file.
fn public(mut t: Transfer) -> Transfer {
    t.local_path = None;
    t
}

fn public_all(list: Vec<Transfer>) -> Vec<Transfer> {
    list.into_iter().map(public).collect()
}

async fn downloads(State(ctx): State<Ctx>) -> Json<Vec<Transfer>> {
    Json(public_all(ctx.client.transfers(Direction::Download)))
}

async fn uploads(State(ctx): State<Ctx>) -> Json<Vec<Transfer>> {
    Json(public_all(ctx.client.transfers(Direction::Upload)))
}

async fn add_downloads(State(ctx): State<Ctx>, Json(b): Json<DownloadBody>) -> R<Vec<Transfer>> {
    if b.items.is_empty() {
        return Err(ApiError::bad_request("no items"));
    }
    Ok(Json(public_all(ctx.client.add_downloads(b.items, b.destination)?)))
}

fn cancel(ctx: &Ctx, dir: Direction, id: &str, remove: bool) -> Empty {
    let t = found(ctx.client.transfer(id), "transfer")?;
    if t.direction != dir {
        return Err(ApiError::not_found("no such transfer"));
    }
    ctx.client.cancel_transfer(id, remove);
    NO_CONTENT
}

async fn cancel_download(State(ctx): State<Ctx>, Path(id): Path<String>, Query(q): Query<RemoveQuery>) -> Empty {
    cancel(&ctx, Direction::Download, &id, q.remove.unwrap_or(false))
}

async fn cancel_upload(State(ctx): State<Ctx>, Path(id): Path<String>, Query(q): Query<RemoveQuery>) -> Empty {
    cancel(&ctx, Direction::Upload, &id, q.remove.unwrap_or(false))
}

fn clear(ctx: &Ctx, dir: Direction, q: ClearQuery) -> Empty {
    let only_succeeded = match q.state.as_deref() {
        None | Some("finished") => false,
        Some("succeeded") => true,
        Some(s) => return Err(ApiError::bad_request(&format!("unknown state {s:?}"))),
    };
    ctx.client.clear_transfers(dir, only_succeeded);
    NO_CONTENT
}

async fn clear_downloads(State(ctx): State<Ctx>, Query(q): Query<ClearQuery>) -> Empty {
    clear(&ctx, Direction::Download, q)
}

async fn clear_uploads(State(ctx): State<Ctx>, Query(q): Query<ClearQuery>) -> Empty {
    clear(&ctx, Direction::Upload, q)
}

async fn retry_download(State(ctx): State<Ctx>, Path(id): Path<String>) -> R<Transfer> {
    Ok(Json(public(ctx.client.retry_download(&id)?)))
}

async fn refresh_position(State(ctx): State<Ctx>, Path(id): Path<String>) -> R<Transfer> {
    Ok(Json(public(ctx.client.refresh_place_in_queue(&id).await?)))
}

// ---------- users ----------

async fn user_info(State(ctx): State<Ctx>, Path(u): Path<String>) -> R<UserInfo> {
    Ok(Json(ctx.client.user_info(&u).await?))
}

async fn user_address(State(ctx): State<Ctx>, Path(u): Path<String>) -> R<Value> {
    let a = ctx.client.user_address(&u).await?;
    Ok(Json(json!({ "ip": a.ip().to_string(), "port": a.port() })))
}

fn image_type(b: &[u8]) -> &'static str {
    if b.starts_with(b"\x89PNG") {
        "image/png"
    } else if b.starts_with(&[0xFF, 0xD8]) {
        "image/jpeg"
    } else if b.starts_with(b"GIF8") {
        "image/gif"
    } else if b.len() > 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        "image/webp"
    } else if b.starts_with(b"BM") {
        "image/bmp"
    } else {
        "application/octet-stream"
    }
}

async fn user_picture(State(ctx): State<Ctx>, Path(u): Path<String>) -> Result<Response, ApiError> {
    let pic = found(ctx.client.user_picture(&u).await, "picture")?;
    Ok((
        [
            (header::CONTENT_TYPE, image_type(&pic)),
            (header::CACHE_CONTROL, "private, max-age=600"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        pic,
    )
        .into_response())
}

async fn user_status(State(ctx): State<Ctx>, Path(u): Path<String>) -> R<UserStatusInfo> {
    Ok(Json(ctx.client.user_status(&u).await?))
}

async fn user_stats(State(ctx): State<Ctx>, Path(u): Path<String>) -> R<UserStats> {
    Ok(Json(ctx.client.user_stats(&u).await?))
}

async fn browse(State(ctx): State<Ctx>, Path(u): Path<String>) -> R<BrowseResult> {
    Ok(Json(ctx.client.browse(&u).await?))
}

#[derive(Deserialize)]
struct DirectoryBody {
    directory: String,
}

async fn directory(State(ctx): State<Ctx>, Path(u): Path<String>, Json(b): Json<DirectoryBody>) -> R<Vec<Directory>> {
    Ok(Json(ctx.client.directory(&u, &b.directory).await?))
}

async fn user_interests(State(ctx): State<Ctx>, Path(u): Path<String>) -> R<Interests> {
    Ok(Json(ctx.client.user_interests(&u).await?))
}

async fn profile(State(ctx): State<Ctx>, Path(u): Path<String>) -> Json<UserProfile> {
    Json(ctx.client.profile(&u))
}

#[derive(Deserialize)]
struct NoteBody {
    note: String,
}

async fn set_note(State(ctx): State<Ctx>, Path(u): Path<String>, Json(b): Json<NoteBody>) -> Json<UserProfile> {
    Json(ctx.client.set_note(&u, &b.note))
}

#[derive(Deserialize)]
struct GroupBody {
    group: Option<String>,
}

async fn set_group(State(ctx): State<Ctx>, Path(u): Path<String>, Json(b): Json<GroupBody>) -> R<UserProfile> {
    Ok(Json(ctx.client.set_group(&u, b.group.as_deref())?))
}

async fn user_countries(State(ctx): State<Ctx>) -> Json<std::collections::HashMap<String, String>> {
    Json(ctx.client.user_countries())
}

async fn reputation(State(ctx): State<Ctx>) -> Json<std::collections::HashMap<String, i32>> {
    Json(ctx.client.reputations())
}

// ---------- conversations ----------

#[derive(Deserialize)]
struct MessageBody {
    message: String,
}

async fn conversations(State(ctx): State<Ctx>) -> Json<Vec<Conversation>> {
    Json(ctx.client.conversations())
}

async fn conversation(State(ctx): State<Ctx>, Path(u): Path<String>) -> Json<Vec<Message>> {
    Json(ctx.client.conversation(&u))
}

async fn send_message(State(ctx): State<Ctx>, Path(u): Path<String>, Json(b): Json<MessageBody>) -> R<Message> {
    Ok(Json(ctx.client.send_message(&u, &b.message)?))
}

async fn mark_read(State(ctx): State<Ctx>, Path(u): Path<String>) -> Empty {
    ctx.client.mark_conversation_read(&u);
    NO_CONTENT
}

async fn delete_conversation(State(ctx): State<Ctx>, Path(u): Path<String>) -> Empty {
    ctx.client.delete_conversation(&u);
    NO_CONTENT
}

// ---------- rooms ----------

#[derive(Deserialize)]
struct NameBody {
    name: String,
}

async fn available_rooms(State(ctx): State<Ctx>) -> R<Vec<RoomSummary>> {
    Ok(Json(ctx.client.available_rooms().await?))
}

async fn joined_rooms(State(ctx): State<Ctx>) -> Json<Vec<Room>> {
    Json(ctx.client.joined_rooms())
}

async fn room(State(ctx): State<Ctx>, Path(name): Path<String>) -> R<Room> {
    Ok(Json(found(ctx.client.room(&name), "room")?))
}

async fn join_room(State(ctx): State<Ctx>, Json(b): Json<NameBody>) -> R<Room> {
    Ok(Json(ctx.client.join_room(&b.name).await?))
}

async fn leave_room(State(ctx): State<Ctx>, Path(name): Path<String>) -> Empty {
    ctx.client.leave_room(&name)?;
    NO_CONTENT
}

async fn say_room(State(ctx): State<Ctx>, Path(name): Path<String>, Json(b): Json<MessageBody>) -> Empty {
    ctx.client.say_room(&name, &b.message)?;
    NO_CONTENT
}

// ---------- shares & files ----------

async fn shares(State(ctx): State<Ctx>) -> Json<ShareSummary> {
    Json(ctx.client.shares.summary())
}

async fn rescan(State(ctx): State<Ctx>) -> Empty {
    ctx.client.rescan_shares();
    NO_CONTENT
}

#[derive(Deserialize)]
struct PathQuery {
    #[serde(default)]
    root: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    download: Option<String>,
}

/// A folder the file manager may browse.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct FileRoot {
    id: String,
    label: String,
    kind: &'static str,
    /// Host path; used internally, never sent to clients.
    #[serde(skip)]
    path: String,
    shared: bool,
    exists: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    file_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    directory_count: Option<usize>,
    #[serde(skip)]
    abs: PathBuf,
}

fn file_roots(ctx: &Ctx) -> (Vec<FileRoot>, Vec<PathBuf>) {
    let c = ctx.client.config();
    let share_roots = c.share_roots(&ctx.client.base_dir);
    let shares = media::canonical_all(&share_roots.iter().map(|(_, p)| p.clone()).collect::<Vec<_>>());
    let summary = ctx.client.shares.summary();
    let mk = |id: String, label: String, kind: &'static str, abs: PathBuf| {
        let canon = abs.canonicalize().ok();
        FileRoot {
            id,
            label,
            kind,
            path: abs.display().to_string(),
            shared: canon.as_deref().is_some_and(|p| media::is_shared(p, &shares)),
            exists: canon.is_some(),
            file_count: None,
            directory_count: None,
            abs,
        }
    };
    let mut out = vec![
        mk("storage".into(), "All files".into(), "storage", c.storage_root(&ctx.client.base_dir)),
        mk("downloads".into(), "Downloads".into(), "downloads", ctx.client.resolve_path(&c.directories.downloads)),
        mk("incomplete".into(), "Incomplete".into(), "incomplete", ctx.client.resolve_path(&c.directories.incomplete)),
    ];
    for d in &c.directories.destinations {
        out.push(mk(format!("dest-{}", d.name), d.name.clone(), "destination", ctx.client.resolve_path(&d.path)));
    }
    for (i, (alias, abs)) in share_roots.into_iter().enumerate() {
        let mut r = mk(format!("share-{i}"), alias, "share", abs);
        if let Some(s) = summary.directories.iter().find(|s| s.path == r.path) {
            r.file_count = Some(s.file_count);
            r.directory_count = Some(s.directory_count);
        }
        out.push(r);
    }
    (out, shares)
}

fn file_root(ctx: &Ctx, id: &str) -> Result<(PathBuf, Vec<PathBuf>), ApiError> {
    let id = if id.is_empty() { "storage" } else { id };
    let (roots, shares) = file_roots(ctx);
    let r = found(roots.into_iter().find(|r| r.id == id), "folder")?;
    Ok((r.abs, shares))
}

async fn list_roots(State(ctx): State<Ctx>) -> Json<Vec<FileRoot>> {
    Json(file_roots(&ctx).0)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct FileListing {
    root: String,
    path: String,
    shared: bool,
    /// Id of the shared root that is exactly this folder, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    share_id: Option<String>,
    entries: Vec<media::FileEntry>,
}

async fn list_files(State(ctx): State<Ctx>, Query(q): Query<PathQuery>) -> R<FileListing> {
    let (root, shares) = file_root(&ctx, &q.root)?;
    let root_id = if q.root.is_empty() { "storage".into() } else { q.root.clone() };
    if q.path.is_empty() && !root.exists() {
        return Ok(Json(FileListing { root: root_id, path: String::new(), shared: false, share_id: None, entries: vec![] }));
    }
    let dir = media::confine(&root, &q.path)?;
    let entries = media::list(&root, &q.path, &shares).await?;
    let share_id = file_roots(&ctx)
        .0
        .into_iter()
        .find(|r| r.kind == "share" && r.abs.canonicalize().ok().as_deref() == Some(dir.as_path()))
        .map(|r| r.id);
    Ok(Json(FileListing { root: root_id, path: q.path, shared: media::is_shared(&dir, &shares), share_id, entries }))
}

async fn delete_file(State(ctx): State<Ctx>, Query(q): Query<PathQuery>) -> Empty {
    let (root, _) = file_root(&ctx, &q.root)?;
    media::delete(&root, &q.path).await?;
    NO_CONTENT
}

#[derive(Deserialize)]
struct MkdirBody {
    #[serde(default)]
    root: String,
    path: String,
}

async fn make_dir(State(ctx): State<Ctx>, Json(b): Json<MkdirBody>) -> Empty {
    let (root, _) = file_root(&ctx, &b.root)?;
    if !root.exists() {
        std::fs::create_dir_all(&root).map_err(|e| ApiError::bad_request(&format!("cannot create root: {e}")))?;
    }
    media::mkdir(&root, &b.path).await?;
    Ok(StatusCode::CREATED)
}

#[derive(Deserialize)]
struct MoveBody {
    #[serde(default)]
    root: String,
    from: String,
    to: String,
}

async fn move_file(State(ctx): State<Ctx>, Json(b): Json<MoveBody>) -> Empty {
    let (root, _) = file_root(&ctx, &b.root)?;
    media::rename(&root, &b.from, &b.to).await?;
    NO_CONTENT
}

/// Multipart upload into `?root=&path=` (a folder). Existing files are never overwritten.
async fn upload_files(State(ctx): State<Ctx>, Query(q): Query<PathQuery>, mut mp: Multipart) -> R<Vec<String>> {
    use tokio::io::AsyncWriteExt;
    let (root, _) = file_root(&ctx, &q.root)?;
    if !root.exists() {
        std::fs::create_dir_all(&root).map_err(|e| ApiError::bad_request(&format!("cannot create root: {e}")))?;
    }
    let dir = q.path.trim_matches('/').to_owned();
    let mut saved = Vec::new();
    while let Some(mut field) = mp.next_field().await.map_err(|e| ApiError::bad_request(&e.to_string()))? {
        let Some(name) = field.file_name().map(|n| n.rsplit(['/', '\\']).next().unwrap_or(n).to_owned()) else { continue };
        let rel = if dir.is_empty() { name.clone() } else { format!("{dir}/{name}") };
        let dest = media::confine_new(&root, &rel)?;
        let mut f = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&dest)
            .await
            .map_err(|e| ApiError::bad_request(&format!("cannot create {name}: {e}")))?;
        let res: Result<(), ApiError> = async {
            while let Some(chunk) = field.chunk().await.map_err(|e| ApiError::bad_request(&e.to_string()))? {
                f.write_all(&chunk).await.map_err(|e| ApiError::internal(&e.to_string()))?;
            }
            f.flush().await.map_err(|e| ApiError::internal(&e.to_string()))
        }
        .await;
        if let Err(e) = res {
            drop(f);
            let _ = tokio::fs::remove_file(&dest).await;
            return Err(e);
        }
        tracing::info!("uploaded {}", dest.display());
        saved.push(rel);
    }
    Ok(Json(saved))
}

async fn stream_file(State(ctx): State<Ctx>, Query(q): Query<PathQuery>, headers: HeaderMap) -> Result<Response, ApiError> {
    let (root, _) = file_root(&ctx, &q.root)?;
    let p = media::confine(&root, &q.path)?;
    media::stream(&p, &headers, q.download.as_deref().is_some_and(|d| d == "1" || d == "true")).await
}

#[derive(Deserialize)]
struct ShareDirBody {
    /// With `root`, a folder relative to that file root; without, a folder
    /// relative to the storage folder (from the folder picker).
    #[serde(default)]
    root: Option<String>,
    #[serde(default)]
    path: String,
    #[serde(default)]
    alias: Option<String>,
}

async fn add_share_dir(State(ctx): State<Ctx>, Json(b): Json<ShareDirBody>) -> R<Vec<FileRoot>> {
    let c = ctx.client.config();
    let base = ctx.client.base_dir.clone();
    let storage = c.storage_root(&base);
    std::fs::create_dir_all(&storage).map_err(|e| ApiError::internal(&format!("cannot create storage folder: {e}")))?;
    let storage = storage.canonicalize().map_err(|e| ApiError::internal(&e.to_string()))?;
    let canon = match b.root.as_deref().filter(|r| !r.is_empty()) {
        Some(root) => media::confine(&file_root(&ctx, root)?.0, &b.path)?,
        None => media::confine(&storage, &b.path)?,
    };
    if !canon.is_dir() {
        return Err(ApiError::bad_request("not a folder"));
    }
    // Confined twice over: `confine` resolves symlinks, and every file root is
    // itself inside storage, but check the final answer directly.
    let rel = canon.strip_prefix(&storage).map_err(|_| ApiError::bad_request("only folders inside storage can be shared"))?.to_owned();
    if rel.as_os_str().is_empty() {
        return Err(ApiError::bad_request("share a folder inside storage, not all of it"));
    }
    if c.resolve(&base, &c.directories.incomplete).canonicalize().ok().is_some_and(|inc| canon.starts_with(&inc)) {
        return Err(ApiError::bad_request("incomplete downloads cannot be shared"));
    }
    if c.share_roots(&base).iter().any(|(_, p)| p.canonicalize().ok().as_deref() == Some(canon.as_path())) {
        return Err(ApiError::bad_request("that folder is already shared"));
    }
    let alias = b.alias.map(|a| a.trim().to_owned()).filter(|a| !a.is_empty());
    if alias.as_deref().is_some_and(|a| a.contains([']', '\\', '/'])) {
        return Err(ApiError::bad_request("alias cannot contain ], / or \\"));
    }
    let dir_name = canon.file_name().map(|n| n.to_string_lossy().into_owned());
    let spec = match alias {
        Some(a) if Some(&a) != dir_name.as_ref() => format!("[{a}]{}", rel.display()),
        _ => rel.display().to_string(),
    };
    ctx.client.update_config(|c| c.shares.directories.push(spec))?;
    Ok(Json(file_roots(&ctx).0))
}

/// Stop sharing the shared root `root` (an id from `/files/roots`).
async fn remove_share_dir(State(ctx): State<Ctx>, Query(q): Query<PathQuery>) -> R<Vec<FileRoot>> {
    let index: usize = q
        .root
        .strip_prefix("share-")
        .and_then(|n| n.parse().ok())
        .ok_or_else(|| ApiError::bad_request("root must be a shared folder id"))?;
    let cfg = ctx.client.config();
    // `share-N` counts the non-empty entries, as `share_roots` does.
    let specs: Vec<&String> = cfg.shares.directories.iter().filter(|s| !s.trim().is_empty()).collect();
    if index >= specs.len() {
        return Err(ApiError::not_found("that folder is not shared"));
    }
    let keep: Vec<String> = specs.iter().enumerate().filter(|(i, _)| *i != index).map(|(_, s)| (*s).clone()).collect();
    ctx.client.update_config(|c| c.shares.directories = keep)?;
    Ok(Json(file_roots(&ctx).0))
}

/// Folders inside storage, for the share picker.
async fn host_dirs(State(ctx): State<Ctx>, Query(q): Query<PathQuery>) -> R<media::HostListing> {
    let c = ctx.client.config();
    let storage = c.storage_root(&ctx.client.base_dir);
    std::fs::create_dir_all(&storage).map_err(|e| ApiError::internal(&format!("cannot create storage folder: {e}")))?;
    let incomplete = c.resolve(&ctx.client.base_dir, &c.directories.incomplete);
    Ok(Json(media::storage_dirs(&storage, &q.path, Some(&incomplete)).await?))
}

async fn stream_transfer(State(ctx): State<Ctx>, Path(id): Path<String>, headers: HeaderMap) -> Result<Response, ApiError> {
    let t = found(ctx.client.transfer(&id), "transfer")?;
    if t.direction != Direction::Download || t.state != TransferState::Succeeded {
        return Err(ApiError::bad_request("transfer is not a completed download"));
    }
    let p = found(t.local_path, "file")?;
    media::stream(std::path::Path::new(&p), &headers, false).await
}

// ---------- wishlist ----------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WishlistBody {
    query: String,
    #[serde(default)]
    auto_download: bool,
    #[serde(default)]
    filter: WishlistFilter,
}

async fn wishlist(State(ctx): State<Ctx>) -> Json<Vec<WishlistItem>> {
    Json(ctx.client.wishlist())
}

async fn add_wishlist(State(ctx): State<Ctx>, Json(b): Json<WishlistBody>) -> R<WishlistItem> {
    Ok(Json(ctx.client.add_wishlist(&b.query, b.auto_download, b.filter, None)?))
}

async fn update_wishlist(State(ctx): State<Ctx>, Path(id): Path<String>, Json(b): Json<WishlistPatch>) -> R<WishlistItem> {
    Ok(Json(ctx.client.update_wishlist(&id, b)?))
}

async fn delete_wishlist(State(ctx): State<Ctx>, Path(id): Path<String>) -> Empty {
    if !ctx.client.delete_wishlist(&id) {
        return Err(ApiError::not_found("no such wishlist item"));
    }
    NO_CONTENT
}

async fn run_wishlist(State(ctx): State<Ctx>, Path(id): Path<String>) -> R<WishlistItem> {
    Ok(Json(ctx.client.run_wishlist_item(&id)?))
}

// ---------- discovery ----------

#[derive(Deserialize)]
struct InterestBody {
    item: String,
    kind: String,
}

impl InterestBody {
    fn hate(&self) -> Result<bool, ApiError> {
        match self.kind.as_str() {
            "like" => Ok(false),
            "hate" => Ok(true),
            k => Err(ApiError::bad_request(&format!("kind must be like or hate, not {k:?}"))),
        }
    }
}

async fn interests(State(ctx): State<Ctx>) -> Json<Interests> {
    Json(ctx.client.interests())
}

async fn add_interest(State(ctx): State<Ctx>, Json(b): Json<InterestBody>) -> R<Interests> {
    Ok(Json(ctx.client.add_interest(&b.item, b.hate()?)?))
}

async fn remove_interest(State(ctx): State<Ctx>, Json(b): Json<InterestBody>) -> R<Interests> {
    Ok(Json(ctx.client.remove_interest(&b.item, b.hate()?)))
}

async fn recommendations(State(ctx): State<Ctx>) -> R<Recommendations> {
    Ok(Json(ctx.client.recommendations().await?))
}

async fn global_recommendations(State(ctx): State<Ctx>) -> R<Recommendations> {
    Ok(Json(ctx.client.global_recommendations().await?))
}

async fn similar_users(State(ctx): State<Ctx>) -> R<Vec<SimilarUser>> {
    Ok(Json(ctx.client.similar_users().await?))
}

async fn item_info(State(ctx): State<Ctx>, Path(item): Path<String>) -> R<ItemInfo> {
    Ok(Json(ctx.client.item_info(&item).await?))
}

// ---------- now playing ----------

#[derive(Deserialize)]
struct NowPlayingBody {
    #[serde(default)]
    artist: String,
    #[serde(default)]
    title: String,
    album: Option<String>,
}

async fn now_playing(State(ctx): State<Ctx>) -> Json<Option<NowPlaying>> {
    Json(ctx.client.now_playing())
}

async fn set_now_playing(State(ctx): State<Ctx>, Json(b): Json<NowPlayingBody>) -> R<NowPlaying> {
    Ok(Json(ctx.client.set_now_playing(&b.artist, &b.title, b.album.as_deref(), "api")?))
}

async fn clear_now_playing(State(ctx): State<Ctx>) -> Empty {
    ctx.client.clear_now_playing();
    NO_CONTENT
}

async fn jellyfin_webhook(State(ctx): State<Ctx>, Json(v): Json<Value>) -> Empty {
    ctx.client.jellyfin_webhook(&v)?;
    NO_CONTENT
}

async fn plex_webhook(State(ctx): State<Ctx>, mut mp: Multipart) -> Empty {
    while let Some(field) = mp.next_field().await.map_err(|e| ApiError::bad_request(&e.to_string()))? {
        if field.name() == Some("payload") {
            let text = field.text().await.map_err(|e| ApiError::bad_request(&e.to_string()))?;
            let v: Value = serde_json::from_str(&text).map_err(|e| ApiError::bad_request(&format!("bad payload: {e}")))?;
            ctx.client.plex_webhook(&v)?;
            return NO_CONTENT;
        }
    }
    Err(ApiError::bad_request("missing payload field"))
}

// ---------- integrations ----------

async fn lidarr_sync(State(ctx): State<Ctx>) -> R<Value> {
    let added = ctx.client.lidarr_sync().await?;
    Ok(Json(json!({ "added": added })))
}

async fn lidarr_status(State(ctx): State<Ctx>) -> Json<LidarrStatus> {
    Json(ctx.client.lidarr_status())
}

// ---------- system ----------

async fn options(State(ctx): State<Ctx>) -> Json<Config> {
    Json(ctx.client.config().redacted())
}

async fn save_options(State(ctx): State<Ctx>, Json(incoming): Json<Config>) -> R<Config> {
    let merged = ctx.client.config().merge_redacted(incoming);
    if merged.web.auth.password.is_empty() && !merged.web.auth.disabled {
        return Err(ApiError::bad_request("a web password is required unless authentication is disabled"));
    }
    {
        use cseek_core::config::is_storage_relative;
        let d = &merged.directories;
        let paths = [&d.downloads, &d.incomplete].into_iter().chain(d.destinations.iter().map(|x| &x.path));
        let shares = merged.shares.directories.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).map(|s| match s.strip_prefix('[').and_then(|r| r.split_once(']')) {
            Some((_, p)) => p.trim(),
            None => s,
        });
        for p in paths.map(|p| p.as_str()).chain(shares) {
            if !is_storage_relative(p) {
                return Err(ApiError::bad_request(&format!("{p:?} must be a folder inside storage (a relative path without ..)")));
            }
        }
    }
    let mut names = std::collections::HashSet::new();
    for d in &merged.directories.destinations {
        if d.name.trim().is_empty() || d.path.trim().is_empty() || !names.insert(d.name.clone()) {
            return Err(ApiError::bad_request("destinations need unique names and a path"));
        }
    }
    let mut groups = std::collections::HashSet::new();
    for g in &merged.groups {
        let n = g.name.trim();
        if n.is_empty() || n == GROUP_BLACKLISTED || n == GROUP_LEECHERS || n == "default" || n == "privileged" || !groups.insert(n.to_owned()) {
            return Err(ApiError::bad_request(&format!("invalid or duplicate group name {:?}", g.name)));
        }
    }
    ctx.client.set_config(merged)?;
    Ok(Json(ctx.client.config().redacted()))
}

async fn logs(State(ctx): State<Ctx>) -> Json<Vec<LogEntry>> {
    Json(ctx.logs.entries())
}

async fn metrics_json(State(ctx): State<Ctx>) -> Json<Metrics> {
    Json(ctx.client.metrics())
}

async fn guard(State(ctx): State<Ctx>) -> Json<GuardState> {
    Json(ctx.client.guard_state())
}

async fn unban(State(ctx): State<Ctx>, Path(u): Path<String>) -> Empty {
    ctx.client.unban(&u);
    NO_CONTENT
}

async fn groups(State(ctx): State<Ctx>) -> Json<Value> {
    Json(json!({ "builtin": [GROUP_BLACKLISTED, GROUP_LEECHERS], "custom": ctx.client.custom_groups() }))
}

async fn prometheus(State(ctx): State<Ctx>) -> impl IntoResponse {
    let m = ctx.client.metrics();
    let a = ctx.client.app_state();
    let mut out = String::new();
    let mut metric = |name: &str, kind: &str, help: &str, v: f64| {
        out.push_str(&format!("# HELP cseek_{name} {help}\n# TYPE cseek_{name} {kind}\ncseek_{name} {v}\n"));
    };
    metric("uptime_seconds", "gauge", "Seconds since the daemon started.", m.uptime_seconds as f64);
    metric("bytes_downloaded_total", "counter", "Bytes downloaded.", m.bytes_downloaded as f64);
    metric("bytes_uploaded_total", "counter", "Bytes uploaded.", m.bytes_uploaded as f64);
    metric("downloads_succeeded_total", "counter", "Downloads completed.", m.downloads_succeeded as f64);
    metric("downloads_failed_total", "counter", "Downloads failed.", m.downloads_failed as f64);
    metric("uploads_succeeded_total", "counter", "Uploads completed.", m.uploads_succeeded as f64);
    metric("uploads_failed_total", "counter", "Uploads failed.", m.uploads_failed as f64);
    metric("searches_sent_total", "counter", "Searches we started.", m.searches_sent as f64);
    metric("search_requests_received_total", "counter", "Search requests received from the network.", m.search_requests_received as f64);
    metric("search_responses_sent_total", "counter", "Search results we sent.", m.search_responses_sent as f64);
    metric("peer_connections_total", "counter", "Peer connections established.", m.peer_connections as f64);
    metric("download_speed_bytes", "gauge", "Current download speed (bytes/s).", a.transfers.download_speed);
    metric("upload_speed_bytes", "gauge", "Current upload speed (bytes/s).", a.transfers.upload_speed);
    metric("active_downloads", "gauge", "Active downloads.", a.transfers.active_downloads as f64);
    metric("active_uploads", "gauge", "Active uploads.", a.transfers.active_uploads as f64);
    metric("queued_uploads", "gauge", "Queued uploads.", a.transfers.queued_uploads as f64);
    metric("shared_files", "gauge", "Shared files.", a.shares.file_count as f64);
    metric("shared_directories", "gauge", "Shared directories.", a.shares.directory_count as f64);
    metric("logged_in", "gauge", "1 when logged in to the Soulseek server.", (a.server.state == ServerState::LoggedIn) as u8 as f64);
    metric("distributed_has_parent", "gauge", "1 when connected to a distributed parent.", a.distributed.has_parent as u8 as f64);
    metric("unread_messages", "gauge", "Unread private messages.", a.unread_messages as f64);
    ([(header::CONTENT_TYPE, "text/plain; version=0.0.4")], out)
}

// ---------- events ----------

async fn events(State(ctx): State<Ctx>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| event_loop(ctx, socket))
}

async fn event_loop(ctx: Ctx, mut socket: WebSocket) {
    let mut events = ctx.client.subscribe();
    let mut logs = ctx.logs.subscribe();
    let send = |e: &Event| serde_json::to_string(e).ok().map(|s| WsMessage::Text(s.into()));
    if let Some(m) = send(&Event::App(Box::new(ctx.client.app_state()))) {
        if socket.send(m).await.is_err() {
            return;
        }
    }
    use tokio::sync::broadcast::error::RecvError;
    loop {
        let msg = tokio::select! {
            e = events.recv() => match e {
                Ok(Event::TransferUpdated(t)) => send(&Event::TransferUpdated(Box::new(public(*t)))),
                Ok(e) => send(&e),
                // a slow client missed events; resync the app state
                Err(RecvError::Lagged(_)) => send(&Event::App(Box::new(ctx.client.app_state()))),
                Err(RecvError::Closed) => break,
            },
            l = logs.recv() => match l {
                Ok(l) => send(&Event::Log(l)),
                Err(RecvError::Lagged(_)) => None,
                Err(RecvError::Closed) => break,
            },
            incoming = socket.recv() => match incoming {
                Some(Ok(WsMessage::Close(_))) | None | Some(Err(_)) => break,
                _ => None,
            },
        };
        if let Some(m) = msg {
            if socket.send(m).await.is_err() {
                break;
            }
        }
    }
}
