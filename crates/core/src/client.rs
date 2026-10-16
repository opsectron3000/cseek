//! The client: owns all state and the server session. Domain logic lives in
//! sibling modules as further `impl Client` blocks.

use crate::config::Config;
use crate::db::Db;
use crate::distributed::DistState;
use crate::geoip::{Countries, CountrySource};
use crate::guard::Guard;
use crate::model::*;
use crate::net::{read_frame, Limiter, RateMeter};
use crate::peers::{PeerHandle, PendingIndirect};
use crate::rooms::RoomsState;
use crate::search::Searches;
use crate::shares::Shares;
use crate::transfers::Transfers;
use anyhow::{anyhow, bail, Context, Result};
use parking_lot::{Mutex, RwLock};
use slsk_proto::{server::code as sc, PeerMessage, ServerRequest, ServerResponse, MAX_FRAME};
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::{broadcast, mpsc, oneshot, Notify};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A value delivered to someone waiting for a server or peer reply.
#[derive(Debug)]
pub enum Waited {
    Server(ServerResponse),
    Peer(PeerMessage),
}

#[derive(Default)]
pub struct Counters {
    pub downloads_succeeded: AtomicU64,
    pub downloads_failed: AtomicU64,
    pub uploads_succeeded: AtomicU64,
    pub uploads_failed: AtomicU64,
    pub searches_sent: AtomicU64,
    pub search_requests_received: AtomicU64,
    pub search_responses_sent: AtomicU64,
    pub peer_connections: AtomicU64,
}

pub(crate) struct ServerSession {
    pub state: ServerState,
    pub username: Option<String>,
    pub error: Option<String>,
    pub privileged_seconds: u32,
    pub status: UserStatus,
    pub tx: Option<mpsc::UnboundedSender<Vec<u8>>>,
}

pub struct Client {
    pub(crate) config: RwLock<Config>,
    pub config_path: PathBuf,
    pub base_dir: PathBuf,
    pub db: Db,
    events: broadcast::Sender<Event>,
    pub(crate) started: Instant,
    pub counters: Counters,
    pub(crate) server: Mutex<ServerSession>,
    want_connected: AtomicBool,
    wake: Notify,
    disconnect: Notify,
    waiters: Mutex<HashMap<String, Vec<oneshot::Sender<Waited>>>>,
    next_token: AtomicU32,
    pub(crate) pending_indirect: Mutex<HashMap<u32, PendingIndirect>>,
    pub(crate) peers: Mutex<HashMap<String, PeerHandle>>,
    pub(crate) dial_locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    pub(crate) file_waiters: Mutex<HashMap<(String, u32), oneshot::Sender<TcpStream>>>,
    pub(crate) user_status: Mutex<HashMap<String, (UserStatus, bool)>>,
    pub(crate) watched: Mutex<HashSet<String>>,
    pub(crate) privileged_users: Mutex<HashSet<String>>,
    pub(crate) user_stats: Mutex<HashMap<String, (UserStats, Instant)>>,
    pub(crate) excluded_phrases: Mutex<Vec<String>>,
    pub(crate) now_playing: Mutex<Option<NowPlaying>>,
    pub(crate) pictures: Mutex<HashMap<String, Vec<u8>>>,
    pub(crate) countries: Countries,
    pub shares: Shares,
    pub(crate) searches: Searches,
    pub(crate) transfers: Transfers,
    pub(crate) rooms: RoomsState,
    pub(crate) dist: Mutex<DistState>,
    pub(crate) guard: Guard,
    pub download_limiter: Limiter,
    pub upload_limiter: Limiter,
    pub download_meter: RateMeter,
    pub upload_meter: RateMeter,
    pub(crate) lidarr: Mutex<crate::lidarr::LidarrStatus>,
    pub(crate) wishlist_lock: tokio::sync::Mutex<()>,
}

impl Client {
    /// Create a client. `config_path` is where config edits are saved; `data_dir` holds the database.
    pub fn new(config: Config, config_path: PathBuf, data_dir: PathBuf) -> Result<Arc<Self>> {
        let db = Db::open(&data_dir.join("cseek.db")).context("opening database")?;
        Self::with_db(config, config_path, data_dir, db)
    }

    pub fn with_db(config: Config, config_path: PathBuf, base_dir: PathBuf, db: Db) -> Result<Arc<Self>> {
        let (events, _) = broadcast::channel(4096);
        let dl = config.transfers.download_speed_limit_kbps * 1024;
        let ul = config.transfers.upload_speed_limit_kbps * 1024;
        let client = Arc::new(Self {
            config: RwLock::new(config),
            config_path,
            base_dir,
            db,
            events,
            started: Instant::now(),
            counters: Counters::default(),
            server: Mutex::new(ServerSession {
                state: ServerState::Disconnected,
                username: None,
                error: None,
                privileged_seconds: 0,
                status: UserStatus::Online,
                tx: None,
            }),
            want_connected: AtomicBool::new(false),
            wake: Notify::new(),
            disconnect: Notify::new(),
            waiters: Mutex::new(HashMap::new()),
            next_token: AtomicU32::new(rand::random::<u32>() >> 8),
            pending_indirect: Mutex::new(HashMap::new()),
            peers: Mutex::new(HashMap::new()),
            dial_locks: Mutex::new(HashMap::new()),
            file_waiters: Mutex::new(HashMap::new()),
            user_status: Mutex::new(HashMap::new()),
            watched: Mutex::new(HashSet::new()),
            privileged_users: Mutex::new(HashSet::new()),
            user_stats: Mutex::new(HashMap::new()),
            excluded_phrases: Mutex::new(Vec::new()),
            now_playing: Mutex::new(None),
            pictures: Mutex::new(HashMap::new()),
            countries: Countries::default(),
            shares: Shares::default(),
            searches: Searches::default(),
            transfers: Transfers::new(),
            rooms: RoomsState::default(),
            dist: Mutex::new(DistState::default()),
            guard: Guard::default(),
            download_limiter: Limiter::new(dl),
            upload_limiter: Limiter::new(ul),
            download_meter: RateMeter::default(),
            upload_meter: RateMeter::default(),
            lidarr: Mutex::new(Default::default()),
            wishlist_lock: tokio::sync::Mutex::new(()),
        });
        client.guard.set_enabled(client.config.read().guard.enabled);
        client.guard.load(&client.db);
        client.load_transfers();
        client.load_searches();
        *client.now_playing.lock() = client.db.kv_get("nowPlaying").and_then(|j| serde_json::from_str(&j).ok());
        Ok(client)
    }

    pub fn config(&self) -> Config {
        self.config.read().clone()
    }

    /// Replace the config, persist it and apply what can be applied at runtime.
    pub fn set_config(self: &Arc<Self>, new: Config) -> Result<()> {
        let old = self.config();
        new.save(&self.config_path).context("saving config")?;
        *self.config.write() = new.clone();
        self.download_limiter.set_rate(new.transfers.download_speed_limit_kbps * 1024);
        self.upload_limiter.set_rate(new.transfers.upload_speed_limit_kbps * 1024);
        if old.shares.directories != new.shares.directories || old.shares.exclude_filters != new.shares.exclude_filters {
            self.rescan_shares();
        }
        if old.soulseek.description != new.soulseek.description {
            tracing::info!("description updated");
        }
        if old.soulseek.username != new.soulseek.username
            || old.soulseek.password != new.soulseek.password
            || old.soulseek.server_host != new.soulseek.server_host
            || old.soulseek.server_port != new.soulseek.server_port
        {
            if self.want_connected.load(Ordering::SeqCst) {
                self.disconnect.notify_waiters();
            }
            if new.soulseek.auto_connect {
                self.connect();
            }
        }
        // blacklist additions take effect immediately
        for u in new.blacklist.iter().filter(|u| !old.blacklist.contains(u)) {
            self.cancel_user_transfers(u, "Banned");
        }
        self.guard.set_enabled(new.guard.enabled);
        if old.geoip.enabled != new.geoip.enabled {
            self.geoip_enabled_changed(new.geoip.enabled);
        }
        self.notify_upload_scheduler();
        self.emit_app();
        Ok(())
    }

    pub fn update_config(self: &Arc<Self>, f: impl FnOnce(&mut Config)) -> Result<()> {
        let mut c = self.config();
        f(&mut c);
        self.set_config(c)
    }

    pub fn resolve_path(&self, p: &str) -> PathBuf {
        self.config.read().resolve(&self.base_dir, p)
    }

    // ---------- events ----------

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }

    pub(crate) fn emit(&self, e: Event) {
        let _ = self.events.send(e);
    }

    pub fn emit_app(&self) {
        self.emit(Event::App(Box::new(self.app_state())));
    }

    pub fn app_state(&self) -> AppState {
        let cfg = self.config.read();
        let s = self.server.lock();
        let dist = self.dist.lock();
        AppState {
            version: VERSION.into(),
            server: ServerInfo {
                state: s.state,
                username: s.username.clone(),
                address: format!("{}:{}", cfg.soulseek.server_host, cfg.soulseek.server_port),
                error: s.error.clone(),
                is_privileged: s.privileged_seconds > 0,
                privilege_seconds_left: s.privileged_seconds,
                status: if s.state == ServerState::LoggedIn { s.status } else { UserStatus::Offline },
                listen_port: cfg.soulseek.listen_port,
            },
            shares: self.shares.summary(),
            distributed: DistributedInfo {
                has_parent: dist.parent.is_some(),
                parent: dist.parent.clone(),
                branch_level: dist.branch_level,
                branch_root: dist.branch_root.clone(),
                searches_received: dist.searches_received,
            },
            transfers: self.transfer_summary(),
            now_playing: self.now_playing.lock().clone(),
            unread_messages: self.db.unread_total(),
        }
    }

    pub fn metrics(&self) -> Metrics {
        let c = &self.counters;
        let l = |a: &AtomicU64| a.load(Ordering::Relaxed);
        Metrics {
            uptime_seconds: self.started.elapsed().as_secs(),
            bytes_downloaded: self.download_meter.total(),
            bytes_uploaded: self.upload_meter.total(),
            downloads_succeeded: l(&c.downloads_succeeded),
            downloads_failed: l(&c.downloads_failed),
            uploads_succeeded: l(&c.uploads_succeeded),
            uploads_failed: l(&c.uploads_failed),
            searches_sent: l(&c.searches_sent),
            search_requests_received: l(&c.search_requests_received),
            search_responses_sent: l(&c.search_responses_sent),
            peer_connections: l(&c.peer_connections),
        }
    }

    // ---------- tokens & waiters ----------

    pub(crate) fn token(&self) -> u32 {
        self.next_token.fetch_add(1, Ordering::Relaxed)
    }

    pub(crate) fn register(&self, key: String) -> oneshot::Receiver<Waited> {
        let (tx, rx) = oneshot::channel();
        self.waiters.lock().entry(key).or_default().push(tx);
        rx
    }

    /// Resolve every waiter for `key`. Returns whether anyone was waiting.
    pub(crate) fn complete(&self, key: &str, make: impl Fn() -> Waited) -> bool {
        let Some(list) = self.waiters.lock().remove(key) else { return false };
        let mut any = false;
        for tx in list {
            any |= tx.send(make()).is_ok();
        }
        any
    }

    pub(crate) async fn wait(rx: oneshot::Receiver<Waited>, t: Duration, what: &str) -> Result<Waited> {
        match tokio::time::timeout(t, rx).await {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(_)) => bail!("{what}: cancelled"),
            Err(_) => bail!("{what}: timed out"),
        }
    }

    /// Send a server request and wait for the reply keyed by `key`.
    pub(crate) async fn server_request(&self, req: ServerRequest, key: String, t: Duration) -> Result<ServerResponse> {
        let rx = self.register(key.clone());
        self.server_send(req)?;
        match Self::wait(rx, t, &key).await? {
            Waited::Server(r) => Ok(r),
            Waited::Peer(_) => bail!("unexpected reply"),
        }
    }

    // ---------- server session ----------

    pub fn username(&self) -> Option<String> {
        self.server.lock().username.clone()
    }

    pub fn is_logged_in(&self) -> bool {
        self.server.lock().state == ServerState::LoggedIn
    }

    pub fn server_send(&self, req: ServerRequest) -> Result<()> {
        let s = self.server.lock();
        match (&s.tx, s.state) {
            (Some(tx), ServerState::LoggedIn) => tx.send(req.encode()).map_err(|_| anyhow!("server connection closed")),
            (Some(tx), _) if matches!(req, ServerRequest::Login { .. }) => {
                tx.send(req.encode()).map_err(|_| anyhow!("server connection closed"))
            }
            _ => bail!("not connected to the Soulseek server"),
        }
    }

    /// Ask the supervisor to connect (and keep reconnecting).
    pub fn connect(&self) {
        self.want_connected.store(true, Ordering::SeqCst);
        self.wake.notify_waiters();
    }

    pub fn disconnect(&self) {
        self.want_connected.store(false, Ordering::SeqCst);
        self.disconnect.notify_waiters();
    }

    fn set_server_state(&self, state: ServerState, error: Option<String>) {
        {
            let mut s = self.server.lock();
            s.state = state;
            if error.is_some() || state == ServerState::LoggedIn {
                s.error = error;
            }
            if state != ServerState::LoggedIn {
                s.tx = None;
            }
        }
        self.emit_app();
    }

    /// Start background tasks: listener, server supervisor, schedulers, periodic jobs.
    pub async fn start(self: &Arc<Self>) -> Result<()> {
        let port = self.config.read().soulseek.listen_port;
        let listener = tokio::net::TcpListener::bind(("0.0.0.0", port))
            .await
            .with_context(|| format!("binding Soulseek listen port {port}"))?;
        tracing::info!("listening for peers on port {port}");
        tokio::spawn(self.clone().accept_loop(listener));
        tokio::spawn(self.clone().supervisor());
        tokio::spawn(self.clone().upload_scheduler());
        tokio::spawn(self.clone().ticker());
        tokio::spawn(self.clone().wishlist_loop());
        tokio::spawn(self.clone().lidarr_loop());
        tokio::spawn(self.clone().share_rescan_loop());
        tokio::spawn(self.clone().geoip_loop());
        self.rescan_shares();
        if self.config.read().soulseek.auto_connect {
            self.connect();
        }
        Ok(())
    }

    async fn supervisor(self: Arc<Self>) {
        let mut backoff = 5u64;
        loop {
            let has_creds = {
                let c = self.config.read();
                !c.soulseek.username.is_empty() && !c.soulseek.password.is_empty()
            };
            if !self.want_connected.load(Ordering::SeqCst) || !has_creds {
                if self.want_connected.load(Ordering::SeqCst) && !has_creds {
                    let mut s = self.server.lock();
                    if s.error.is_none() {
                        s.error = Some("No Soulseek username/password configured".into());
                    }
                }
                tokio::select! {
                    _ = self.wake.notified() => {}
                    _ = tokio::time::sleep(Duration::from_secs(2)) => {}
                }
                continue;
            }
            let logged_in_before = Instant::now();
            let result = self.session().await;
            let was_up_long = logged_in_before.elapsed() > Duration::from_secs(60);
            match result {
                Ok(SessionEnd::Requested) => {
                    self.set_server_state(ServerState::Disconnected, None);
                    backoff = 5;
                    continue;
                }
                Ok(SessionEnd::Relogged) => {
                    self.want_connected.store(false, Ordering::SeqCst);
                    self.set_server_state(
                        ServerState::Disconnected,
                        Some("Logged in from another location; not reconnecting".into()),
                    );
                    continue;
                }
                Ok(SessionEnd::Closed) => {
                    tracing::warn!("server connection closed");
                    self.set_server_state(ServerState::Disconnected, Some("Connection to server lost".into()));
                }
                Err(SessionError::LoginRejected(reason)) => {
                    tracing::error!("login rejected: {reason}");
                    self.set_server_state(ServerState::LoginFailed, Some(reason.clone()));
                    if reason.contains("INVALIDPASS") || reason.contains("INVALIDUSERNAME") {
                        self.want_connected.store(false, Ordering::SeqCst);
                        continue;
                    }
                }
                Err(SessionError::Other(e)) => {
                    tracing::warn!("server session error: {e:#}");
                    self.set_server_state(ServerState::Disconnected, Some(format!("{e:#}")));
                }
            }
            self.on_logged_out();
            if was_up_long {
                backoff = 5;
            }
            tracing::info!("reconnecting in {backoff}s");
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(backoff)) => {}
                _ = self.disconnect.notified() => { self.want_connected.store(false, Ordering::SeqCst); }
            }
            backoff = (backoff * 2).min(300);
        }
    }

    async fn session(self: &Arc<Self>) -> std::result::Result<SessionEnd, SessionError> {
        let (host, port, username, password, listen_port) = {
            let c = self.config.read();
            (
                c.soulseek.server_host.clone(),
                c.soulseek.server_port,
                c.soulseek.username.clone(),
                c.soulseek.password.clone(),
                c.soulseek.listen_port,
            )
        };
        self.set_server_state(ServerState::Connecting, None);
        tracing::info!("connecting to {host}:{port}");
        let stream = tokio::time::timeout(Duration::from_secs(20), TcpStream::connect((host.as_str(), port)))
            .await
            .map_err(|_| SessionError::Other(anyhow!("timed out connecting to {host}:{port}")))?
            .map_err(|e| SessionError::Other(anyhow!("connecting to {host}:{port}: {e}")))?;
        let _ = stream.set_nodelay(true);
        let (mut rd, mut wr) = stream.into_split();
        let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let writer = tokio::spawn(async move {
            while let Some(buf) = rx.recv().await {
                if wr.write_all(&buf).await.is_err() {
                    break;
                }
            }
        });
        struct AbortOnDrop(tokio::task::JoinHandle<()>);
        impl Drop for AbortOnDrop {
            fn drop(&mut self) {
                self.0.abort();
            }
        }
        let _writer = AbortOnDrop(writer);

        tx.send(ServerRequest::Login { username: username.clone(), password }.encode()).ok();
        let login = tokio::time::timeout(Duration::from_secs(30), read_frame(&mut rd, MAX_FRAME))
            .await
            .map_err(|_| SessionError::Other(anyhow!("timed out waiting for login response")))?
            .map_err(|e| SessionError::Other(anyhow!("reading login response: {e}")))?;
        match ServerResponse::decode(&login).map_err(|e| SessionError::Other(e.into()))? {
            ServerResponse::LoginSuccess { greeting, ip, .. } => {
                tracing::info!("logged in as {username} (public ip {ip}); {greeting}");
            }
            ServerResponse::LoginFailure { reason } => return Err(SessionError::LoginRejected(reason)),
            other => return Err(SessionError::Other(anyhow!("unexpected login response {other:?}"))),
        }
        {
            let mut s = self.server.lock();
            s.username = Some(username.clone());
            s.tx = Some(tx.clone());
        }
        self.set_server_state(ServerState::LoggedIn, None);
        self.after_login(listen_port);

        let ping_tx = tx.clone();
        let pinger = AbortOnDrop(tokio::spawn(async move {
            let mut iv = tokio::time::interval(Duration::from_secs(300));
            iv.tick().await;
            loop {
                iv.tick().await;
                if ping_tx.send(ServerRequest::ServerPing.encode()).is_err() {
                    break;
                }
            }
        }));
        let _pinger = pinger;

        loop {
            tokio::select! {
                frame = read_frame(&mut rd, MAX_FRAME) => {
                    let frame = match frame {
                        Ok(f) => f,
                        Err(_) => return Ok(SessionEnd::Closed),
                    };
                    match ServerResponse::decode(&frame) {
                        Ok(ServerResponse::Relogged) => return Ok(SessionEnd::Relogged),
                        Ok(msg) => self.handle_server_message(msg),
                        Err(e) => {
                            let code = frame.get(..4).map(|b| u32::from_le_bytes(b.try_into().unwrap())).unwrap_or(0);
                            tracing::debug!("undecodable server message {code}: {e}");
                        }
                    }
                }
                _ = self.disconnect.notified() => return Ok(SessionEnd::Requested),
            }
        }
    }

    fn after_login(self: &Arc<Self>, listen_port: u16) {
        let (dirs, files) = self.shares.counts();
        let username = self.username().unwrap_or_default();
        let status = self.server.lock().status;
        for req in [
            ServerRequest::SetWaitPort { port: listen_port as u32 },
            ServerRequest::SharedFoldersFiles { dirs: dirs as u32, files: files as u32 },
            ServerRequest::SetStatus { status: status.code() },
            ServerRequest::HaveNoParent { value: true },
            ServerRequest::BranchRoot { root: username },
            ServerRequest::BranchLevel { level: 0 },
            ServerRequest::AcceptChildren { value: false },
            ServerRequest::CheckPrivileges,
        ] {
            let _ = self.server_send(req);
        }
        // watch everyone we have a relationship with
        let mut users: HashSet<String> = self.db.conversation_users().into_iter().collect();
        users.extend(self.transfers.usernames());
        for u in users {
            self.watch_user(&u);
        }
        let mut rooms = self.config.read().rooms.auto_join.clone();
        for r in self.saved_rooms() {
            if !rooms.contains(&r) {
                rooms.push(r);
            }
        }
        for room in rooms {
            let _ = self.server_send(ServerRequest::JoinRoom { room });
        }
        self.resend_interests();
        self.resume_downloads();
    }

    fn on_logged_out(&self) {
        self.peers.lock().clear();
        self.pending_indirect.lock().clear();
        *self.dist.lock() = DistState::default();
        self.watched.lock().clear();
    }

    pub fn set_status(&self, status: UserStatus) -> Result<()> {
        self.server.lock().status = status;
        self.server_send(ServerRequest::SetStatus { status: status.code() })?;
        self.emit_app();
        Ok(())
    }

    pub(crate) fn watch_user(&self, username: &str) {
        if self.watched.lock().insert(username.to_owned()) {
            let _ = self.server_send(ServerRequest::WatchUser { username: username.to_owned() });
        }
    }

    fn handle_server_message(self: &Arc<Self>, msg: ServerResponse) {
        use ServerResponse as S;
        match msg {
            S::PeerAddress { username, ip, port } => {
                self.locate_peer(&username, ip.into());
                self.complete(&format!("addr:{username}"), || {
                    Waited::Server(S::PeerAddress { username: username.clone(), ip, port })
                });
            }
            S::ConnectToPeer { username, conn_type, ip, port, token, .. } => {
                self.locate_peer(&username, ip.into());
                let me = self.clone();
                tokio::spawn(async move {
                    me.handle_connect_to_peer(username, conn_type, SocketAddr::from((ip, port as u16)), token).await;
                });
            }
            S::CantConnectToPeer { token } => {
                self.pending_indirect.lock().remove(&token);
            }
            S::WatchUser { username, exists, status, stats, country } => {
                if let Some(c) = country.as_deref().filter(|_| exists) {
                    self.set_country(&username, c, CountrySource::Server);
                }
                if exists {
                    self.user_status.lock().insert(username.clone(), (UserStatus::from_code(status), false));
                    self.user_stats.lock().insert(username.clone(), ((&stats).into(), Instant::now()));
                } else {
                    self.user_status.lock().insert(username.clone(), (UserStatus::Offline, false));
                }
                let m = S::WatchUser { username: username.clone(), exists, status, stats, country };
                self.complete(&format!("watch:{username}"), || Waited::Server(m.clone()));
            }
            S::UserStatus { username, status, privileged } => {
                let st = UserStatus::from_code(status);
                let prev = self.user_status.lock().insert(username.clone(), (st, privileged));
                if privileged {
                    self.privileged_users.lock().insert(username.clone());
                }
                self.complete(&format!("status:{username}"), || {
                    Waited::Server(S::UserStatus { username: username.clone(), status, privileged })
                });
                if prev.map(|p| p.0) != Some(st) {
                    self.on_user_status_changed(&username, st);
                }
            }
            S::UserStats { username, stats } => {
                self.user_stats.lock().insert(username.clone(), ((&stats).into(), Instant::now()));
                self.complete(&format!("stats:{username}"), || {
                    Waited::Server(S::UserStats { username: username.clone(), stats: stats.clone() })
                });
            }
            S::MessageUser { id, timestamp, username, message, is_new } => {
                let _ = self.server_send(ServerRequest::MessageAcked { id });
                self.on_private_message(id, timestamp, username, message, is_new);
            }
            S::FileSearch { username, token, query } => {
                self.handle_search_request(username, token, query);
            }
            S::EmbeddedMessage { distrib_code, payload } => {
                if let Ok(m) = slsk_proto::DistribMessage::decode_body(distrib_code, &payload) {
                    self.handle_distrib_message(None, m);
                }
            }
            S::PossibleParents { parents } => self.on_possible_parents(parents),
            S::ResetDistributed => self.reset_distributed(),
            S::PrivilegedUsers { users } => {
                *self.privileged_users.lock() = users.into_iter().collect();
            }
            S::CheckPrivileges { seconds_left } => {
                self.server.lock().privileged_seconds = seconds_left;
                self.emit_app();
            }
            S::ExcludedSearchPhrases { phrases } => {
                *self.excluded_phrases.lock() = phrases.into_iter().map(|p| p.to_lowercase()).collect();
            }
            S::AdminMessage { message } => tracing::warn!("server admin message: {message}"),
            S::WishlistInterval { seconds } => tracing::debug!("server wishlist interval {seconds}s"),
            S::JoinRoom { .. }
            | S::LeaveRoom { .. }
            | S::UserJoinedRoom { .. }
            | S::UserLeftRoom { .. }
            | S::SayChatroom { .. }
            | S::RoomTickerState { .. }
            | S::RoomTickerAdd { .. }
            | S::RoomTickerRemove { .. }
            | S::RoomList { .. }
            | S::CantCreateRoom { .. } => self.handle_room_message(msg),
            S::Recommendations { .. } => {
                self.complete("recommendations", || Waited::Server(msg.clone()));
            }
            S::GlobalRecommendations { .. } => {
                self.complete("global-recommendations", || Waited::Server(msg.clone()));
            }
            S::SimilarUsers { .. } => {
                self.complete("similar-users", || Waited::Server(msg.clone()));
            }
            S::UserInterests { ref username, .. } => {
                let key = format!("interests:{username}");
                self.complete(&key, || Waited::Server(msg.clone()));
            }
            S::ItemRecommendations { ref item, .. } => {
                let key = format!("item-rec:{}", item.to_lowercase());
                self.complete(&key, || Waited::Server(msg.clone()));
            }
            S::ItemSimilarUsers { ref item, .. } => {
                let key = format!("item-users:{}", item.to_lowercase());
                self.complete(&key, || Waited::Server(msg.clone()));
            }
            S::ParentMinSpeed { .. } | S::ParentSpeedRatio { .. } => {}
            S::LoginSuccess { .. } | S::LoginFailure { .. } | S::Relogged => {}
            S::Unknown { code } => {
                if code != sc::SERVER_PING {
                    tracing::trace!("unhandled server message code {code}");
                }
            }
        }
    }

    /// Periodic housekeeping: speeds, progress events, app state.
    async fn ticker(self: Arc<Self>) {
        let mut iv = tokio::time::interval(Duration::from_millis(1000));
        let mut n = 0u64;
        loop {
            iv.tick().await;
            n += 1;
            self.download_meter.sample();
            self.upload_meter.sample();
            self.flush_transfer_events();
            self.flush_search_events();
            let active = self.transfers.has_active();
            if (active && n % 2 == 0) || n % 15 == 0 {
                self.emit_app();
            }
            if n % 60 == 0 {
                self.guard.expire(&self.db);
                self.persist_active_transfers();
            }
            if n % 3600 == 0 {
                let _ = self.server_send(ServerRequest::CheckPrivileges);
            }
        }
    }
}

enum SessionEnd {
    Requested,
    Relogged,
    Closed,
}

enum SessionError {
    LoginRejected(String),
    Other(anyhow::Error),
}
