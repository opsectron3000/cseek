//! Peer connections: direct + indirect (firewall-piercing) establishment,
//! the listener, and "P" message connections.

use crate::client::{Client, Waited};
use anyhow::{anyhow, bail, Result};
use slsk_proto::init::{CONN_DISTRIBUTED, CONN_FILE, CONN_PEER};
use slsk_proto::{InitMessage, PeerMessage, ServerRequest, ServerResponse};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const INDIRECT_TIMEOUT: Duration = Duration::from_secs(20);
const PEER_IDLE_TIMEOUT: Duration = Duration::from_secs(600);
/// Peer messages are small except share lists; cap anyway.
const MAX_PEER_FRAME: usize = 128 * 1024 * 1024;

static CONN_IDS: AtomicU64 = AtomicU64::new(1);

pub(crate) struct PendingIndirect {
    pub username: String,
    pub tx: oneshot::Sender<TcpStream>,
}

pub(crate) struct PeerHandle {
    pub id: u64,
    pub tx: mpsc::UnboundedSender<Vec<u8>>,
}

impl Client {
    /// The address the server reports for a user (fails when they're offline).
    pub async fn user_address(&self, username: &str) -> Result<SocketAddr> {
        self.get_peer_address(username).await
    }

    pub(crate) async fn get_peer_address(&self, username: &str) -> Result<SocketAddr> {
        let r = self
            .server_request(
                ServerRequest::GetPeerAddress { username: username.to_owned() },
                format!("addr:{username}"),
                Duration::from_secs(10),
            )
            .await?;
        match r {
            ServerResponse::PeerAddress { ip, port, .. } => {
                if ip.is_unspecified() || port == 0 {
                    bail!("{username} is offline");
                }
                Ok(SocketAddr::from((ip, port as u16)))
            }
            _ => bail!("unexpected reply"),
        }
    }

    /// Open a connection of `conn_type` to `username`, trying a direct connection and
    /// an indirect one (the peer connects back to us via the server) concurrently.
    pub(crate) async fn connect_peer(&self, username: &str, conn_type: &str, hint: Option<SocketAddr>) -> Result<TcpStream> {
        let me = self.username().ok_or_else(|| anyhow!("not logged in"))?;
        let token = self.token();
        let (tx, rx) = oneshot::channel();
        self.pending_indirect.lock().insert(token, PendingIndirect { username: username.to_owned(), tx });
        let _ = self.server_send(ServerRequest::ConnectToPeer {
            token,
            username: username.to_owned(),
            conn_type: conn_type.to_owned(),
        });

        let direct = async {
            let addr = match hint {
                Some(a) => a,
                None => self.get_peer_address(username).await?,
            };
            let mut s = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr))
                .await
                .map_err(|_| anyhow!("direct connection to {addr} timed out"))??;
            let _ = s.set_nodelay(true);
            s.write_all(&InitMessage::PeerInit { username: me.clone(), conn_type: conn_type.to_owned(), token }.encode())
                .await?;
            Ok::<_, anyhow::Error>(s)
        };
        let indirect = async {
            match tokio::time::timeout(INDIRECT_TIMEOUT, rx).await {
                Ok(Ok(s)) => Ok(s),
                Ok(Err(_)) => Err(anyhow!("peer could not connect to us")),
                Err(_) => Err(anyhow!("indirect connection timed out")),
            }
        };
        tokio::pin!(direct);
        tokio::pin!(indirect);
        let (mut d_done, mut i_done) = (false, false);
        let mut last_err = anyhow!("connection failed");
        let result = loop {
            tokio::select! {
                r = &mut direct, if !d_done => match r {
                    Ok(s) => break Ok(s),
                    Err(e) => { d_done = true; last_err = e; }
                },
                r = &mut indirect, if !i_done => match r {
                    Ok(s) => break Ok(s),
                    Err(e) => { i_done = true; if d_done { last_err = e; } }
                },
            }
            if d_done && i_done {
                break Err(anyhow!("cannot connect to {username}: {last_err}"));
            }
        };
        self.pending_indirect.lock().remove(&token);
        if result.is_ok() {
            self.counters.peer_connections.fetch_add(1, Ordering::Relaxed);
        }
        result
    }

    /// The server relays a peer's request that we connect to them.
    pub(crate) async fn handle_connect_to_peer(self: &Arc<Self>, username: String, conn_type: String, addr: SocketAddr, token: u32) {
        let attempt = async {
            let mut s = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr)).await??;
            let _ = s.set_nodelay(true);
            s.write_all(&InitMessage::PierceFirewall { token }.encode()).await?;
            Ok::<_, anyhow::Error>(s)
        };
        match attempt.await {
            Ok(stream) => {
                self.counters.peer_connections.fetch_add(1, Ordering::Relaxed);
                self.dispatch_connection(username, conn_type, stream).await;
            }
            Err(e) => {
                tracing::debug!("connect-back to {username} at {addr} failed: {e}");
                let _ = self.server_send(ServerRequest::CantConnectToPeer { token, username });
            }
        }
    }

    pub(crate) async fn accept_loop(self: Arc<Self>, listener: TcpListener) {
        loop {
            let (stream, addr) = match listener.accept().await {
                Ok(x) => x,
                Err(e) => {
                    tracing::warn!("accept failed: {e}");
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    continue;
                }
            };
            let _ = stream.set_nodelay(true);
            let me = self.clone();
            tokio::spawn(async move {
                if let Err(e) = me.handle_incoming(stream).await {
                    tracing::trace!("incoming connection from {addr}: {e}");
                }
            });
        }
    }

    async fn handle_incoming(self: &Arc<Self>, mut stream: TcpStream) -> Result<()> {
        let frame = crate::net::read_frame_timeout(&mut stream, 4096, Duration::from_secs(30)).await?;
        self.counters.peer_connections.fetch_add(1, Ordering::Relaxed);
        match InitMessage::decode(&frame)? {
            InitMessage::PierceFirewall { token } => {
                let pending = self.pending_indirect.lock().remove(&token);
                match pending {
                    Some(p) => {
                        tracing::trace!("indirect connection from {} established", p.username);
                        let _ = p.tx.send(stream);
                    }
                    None => tracing::trace!("unexpected pierce firewall token {token}"),
                }
            }
            InitMessage::PeerInit { username, conn_type, .. } => {
                if let Ok(addr) = stream.peer_addr() {
                    self.locate_peer(&username, addr.ip());
                }
                self.dispatch_connection(username, conn_type, stream).await;
            }
            InitMessage::Unknown { code } => bail!("unknown init code {code}"),
        }
        Ok(())
    }

    async fn dispatch_connection(self: &Arc<Self>, username: String, conn_type: String, mut stream: TcpStream) {
        match conn_type.as_str() {
            CONN_PEER => {
                self.attach_peer(username, stream);
            }
            CONN_FILE => {
                // The uploader always sends the transfer token first.
                let mut tok = [0u8; 4];
                match tokio::time::timeout(Duration::from_secs(30), stream.read_exact(&mut tok)).await {
                    Ok(Ok(_)) => {
                        let token = u32::from_le_bytes(tok);
                        let waiter = self.file_waiters.lock().remove(&(username.clone(), token));
                        match waiter {
                            Some(tx) => {
                                let _ = tx.send(stream);
                            }
                            None => tracing::debug!("unexpected file connection from {username} (token {token})"),
                        }
                    }
                    _ => tracing::debug!("file connection from {username} sent no token"),
                }
            }
            CONN_DISTRIBUTED => {
                // We do not accept distributed children.
                tracing::trace!("ignoring distributed connection from {username}");
            }
            other => tracing::debug!("unknown connection type {other:?} from {username}"),
        }
    }

    /// Register a "P" connection and start reading messages from it.
    pub(crate) fn attach_peer(self: &Arc<Self>, username: String, stream: TcpStream) -> mpsc::UnboundedSender<Vec<u8>> {
        let (mut rd, mut wr) = stream.into_split();
        let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let id = CONN_IDS.fetch_add(1, Ordering::Relaxed);
        self.peers.lock().insert(username.clone(), PeerHandle { id, tx: tx.clone() });
        tokio::spawn(async move {
            while let Some(buf) = rx.recv().await {
                if wr.write_all(&buf).await.is_err() {
                    break;
                }
            }
        });
        let me = self.clone();
        tokio::spawn(async move {
            loop {
                let frame = match crate::net::read_frame_timeout(&mut rd, MAX_PEER_FRAME, PEER_IDLE_TIMEOUT).await {
                    Ok(f) => f,
                    Err(_) => break,
                };
                match PeerMessage::decode(&frame) {
                    Ok(m) => me.handle_peer_message(&username, m),
                    Err(e) => tracing::debug!("bad peer message from {username}: {e}"),
                }
            }
            let mut peers = me.peers.lock();
            if peers.get(&username).map(|h| h.id) == Some(id) {
                peers.remove(&username);
            }
        });
        tx
    }

    async fn peer_tx(self: &Arc<Self>, username: &str) -> Result<mpsc::UnboundedSender<Vec<u8>>> {
        if let Some(h) = self.peers.lock().get(username) {
            if !h.tx.is_closed() {
                return Ok(h.tx.clone());
            }
        }
        let lock = self.dial_locks.lock().entry(username.to_owned()).or_default().clone();
        let _g = lock.lock().await;
        if let Some(h) = self.peers.lock().get(username) {
            if !h.tx.is_closed() {
                return Ok(h.tx.clone());
            }
        }
        let stream = self.connect_peer(username, CONN_PEER, None).await?;
        Ok(self.attach_peer(username.to_owned(), stream))
    }

    /// Send a message to a peer, connecting if needed.
    pub async fn peer_send(self: &Arc<Self>, username: &str, msg: PeerMessage) -> Result<()> {
        let tx = self.peer_tx(username).await?;
        if tx.send(msg.encode()).is_err() {
            // stale connection; retry once with a fresh one
            self.peers.lock().remove(username);
            let tx = self.peer_tx(username).await?;
            tx.send(msg.encode()).map_err(|_| anyhow!("connection to {username} closed"))?;
        }
        Ok(())
    }

    /// Send a peer request and wait for the reply registered under `key`.
    pub(crate) async fn peer_request(self: &Arc<Self>, username: &str, msg: PeerMessage, key: String, t: Duration) -> Result<PeerMessage> {
        let rx = self.register(key.clone());
        self.peer_send(username, msg).await?;
        match Self::wait(rx, t, &key).await? {
            Waited::Peer(m) => Ok(m),
            Waited::Server(_) => bail!("unexpected reply"),
        }
    }

    fn handle_peer_message(self: &Arc<Self>, username: &str, msg: PeerMessage) {
        use PeerMessage as P;
        let u = username.to_owned();
        match msg {
            P::FileSearchResponse(r) => self.on_search_response(r),
            P::UserInfoRequest => {
                let me = self.clone();
                tokio::spawn(async move { me.send_user_info(&u).await });
            }
            P::UserInfoResponse(info) => {
                if let Some(p) = &info.picture {
                    self.pictures.lock().insert(u.clone(), p.clone());
                }
                self.complete(&format!("info:{u}"), || Waited::Peer(P::UserInfoResponse(info.clone())));
            }
            P::SharedFileListRequest => {
                let me = self.clone();
                tokio::spawn(async move { me.send_share_list(&u).await });
            }
            m @ P::SharedFileListResponse { .. } => {
                // big payload: hand it over without cloning per waiter
                let slot = std::sync::Mutex::new(Some(m));
                self.complete(&format!("browse:{u}"), || {
                    Waited::Peer(slot.lock().unwrap().take().unwrap_or(P::Unknown { code: 0 }))
                });
            }
            P::FolderContentsRequest { token, folder } => {
                let me = self.clone();
                tokio::spawn(async move { me.send_folder_contents(&u, token, folder).await });
            }
            P::FolderContentsResponse { token, folder, dirs } => {
                self.complete(&format!("folder:{u}:{token}"), || {
                    Waited::Peer(P::FolderContentsResponse { token, folder: folder.clone(), dirs: dirs.clone() })
                });
            }
            P::TransferRequest { direction, token, filename, size } => {
                self.on_transfer_request(&u, direction, token, filename, size);
            }
            P::TransferResponse { token, allowed, size, reason } => {
                self.complete(&format!("tresp:{u}:{token}"), || {
                    Waited::Peer(P::TransferResponse { token, allowed, size, reason: reason.clone() })
                });
            }
            P::QueueUpload { filename } => {
                let me = self.clone();
                tokio::spawn(async move { me.on_queue_upload(&u, filename).await });
            }
            P::PlaceInQueueRequest { filename } => {
                let me = self.clone();
                tokio::spawn(async move { me.on_place_in_queue_request(&u, filename).await });
            }
            P::PlaceInQueueResponse { filename, place } => self.on_place_in_queue(&u, &filename, place),
            P::UploadFailed { filename } => self.on_remote_upload_failed(&u, &filename, None),
            P::UploadDenied { filename, reason } => self.on_remote_upload_failed(&u, &filename, Some(reason)),
            P::UploadQueueNotification => {}
            P::Unknown { code } => tracing::trace!("unknown peer message {code} from {u}"),
        }
    }
}
