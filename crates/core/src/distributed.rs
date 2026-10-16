//! Distributed network: we join as a leaf (one parent, no children) so search
//! requests reach us without the server having to relay each one.

use crate::client::Client;
use slsk_proto::init::CONN_DISTRIBUTED;
use slsk_proto::{DistribMessage, ServerRequest};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::task::AbortHandle;

const MAX_DISTRIB_FRAME: usize = 4 * 1024 * 1024;
const PARENT_IDLE_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Default)]
pub(crate) struct DistState {
    pub parent: Option<String>,
    pub branch_level: i32,
    pub branch_root: Option<String>,
    pub searches_received: u64,
    pub connecting: bool,
    pub task: Option<AbortHandle>,
}

impl Drop for DistState {
    fn drop(&mut self) {
        if let Some(t) = self.task.take() {
            t.abort();
        }
    }
}

impl Client {
    pub(crate) fn on_possible_parents(self: &Arc<Self>, parents: Vec<(String, Ipv4Addr, u32)>) {
        {
            let mut d = self.dist.lock();
            if d.parent.is_some() || d.connecting {
                return;
            }
            d.connecting = true;
        }
        let me = self.clone();
        tokio::spawn(async move {
            for (username, ip, port) in parents {
                let addr = SocketAddr::from((ip, port as u16));
                match me.connect_peer(&username, CONN_DISTRIBUTED, Some(addr)).await {
                    Ok(stream) => {
                        tracing::info!("distributed parent: {username}");
                        let me2 = me.clone();
                        let u2 = username.clone();
                        let h = tokio::spawn(async move { me2.parent_loop(u2, stream).await });
                        let mut d = me.dist.lock();
                        d.parent = Some(username);
                        d.connecting = false;
                        d.task = Some(h.abort_handle());
                        drop(d);
                        let _ = me.server_send(ServerRequest::HaveNoParent { value: false });
                        me.emit_app();
                        return;
                    }
                    Err(e) => tracing::debug!("distributed parent candidate {username}: {e}"),
                }
            }
            me.dist.lock().connecting = false;
        });
    }

    async fn parent_loop(self: Arc<Self>, username: String, mut stream: tokio::net::TcpStream) {
        loop {
            let frame = match crate::net::read_frame_timeout(&mut stream, MAX_DISTRIB_FRAME, PARENT_IDLE_TIMEOUT).await {
                Ok(f) => f,
                Err(_) => break,
            };
            match DistribMessage::decode(&frame) {
                Ok(m) => self.handle_distrib_message(Some(&username), m),
                Err(e) => tracing::trace!("bad distributed message from {username}: {e}"),
            }
        }
        tracing::info!("lost distributed parent {username}");
        let mine = {
            let mut d = self.dist.lock();
            if d.parent.as_deref() == Some(&username) {
                d.parent = None;
                d.branch_level = 0;
                d.branch_root = None;
                d.task = None;
                true
            } else {
                false
            }
        };
        if mine {
            self.announce_no_parent();
        }
    }

    fn announce_no_parent(&self) {
        let me = self.username().unwrap_or_default();
        for req in [
            ServerRequest::HaveNoParent { value: true },
            ServerRequest::BranchRoot { root: me },
            ServerRequest::BranchLevel { level: 0 },
        ] {
            let _ = self.server_send(req);
        }
        self.emit_app();
    }

    pub(crate) fn reset_distributed(&self) {
        *self.dist.lock() = DistState::default();
        self.announce_no_parent();
    }

    pub(crate) fn handle_distrib_message(self: &Arc<Self>, from: Option<&str>, m: DistribMessage) {
        match m {
            DistribMessage::Search { username, token, query } => {
                self.dist.lock().searches_received += 1;
                self.handle_search_request(username, token, query);
            }
            DistribMessage::BranchLevel { level } if from.is_some() => {
                self.dist.lock().branch_level = level + 1;
                let _ = self.server_send(ServerRequest::BranchLevel { level: (level + 1).max(0) as u32 });
            }
            DistribMessage::BranchRoot { root } if from.is_some() => {
                self.dist.lock().branch_root = Some(root.clone());
                let _ = self.server_send(ServerRequest::BranchRoot { root });
            }
            DistribMessage::EmbeddedMessage { distrib_code, payload } => {
                if let Ok(inner) = DistribMessage::decode_body(distrib_code, &payload) {
                    if !matches!(inner, DistribMessage::EmbeddedMessage { .. }) {
                        self.handle_distrib_message(from, inner);
                    }
                }
            }
            _ => {}
        }
    }
}
