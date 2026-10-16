//! A minimal Soulseek server for local testing. It is not a full
//! implementation: there is no distributed network, so searches are sent
//! directly to every logged-in user.
//!
//! Usage: cseek-mockserver [--port 2242]

use slsk_proto::server::code::*;
use slsk_proto::{frame, Reader, Writer};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

type Tx = mpsc::UnboundedSender<Vec<u8>>;

struct User {
    conn: u64,
    tx: Tx,
    ip: Ipv4Addr,
    port: u32,
    status: u32,
    files: u32,
    dirs: u32,
}

#[derive(Default)]
struct State {
    next_conn: u64,
    next_msg: u32,
    passwords: HashMap<String, String>,
    online: HashMap<String, User>,
    rooms: BTreeMap<String, BTreeSet<String>>,
    likes: HashMap<String, BTreeSet<String>>,
    hates: HashMap<String, BTreeSet<String>>,
    pending_messages: HashMap<String, Vec<(u32, u32, String, String)>>,
}

type Shared = Arc<Mutex<State>>;

fn msg(code: u32, f: impl FnOnce(&mut Writer)) -> Vec<u8> {
    let mut w = Writer::new();
    w.u32(code);
    f(&mut w);
    frame(&w.buf)
}

fn stats(w: &mut Writer, u: Option<&User>) {
    let (files, dirs) = u.map(|u| (u.files, u.dirs)).unwrap_or((0, 0));
    w.u32(100_000).u32(0).u32(0).u32(files).u32(dirs);
}

fn now() -> u32 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as u32).unwrap_or(0)
}

impl State {
    fn send(&self, user: &str, m: Vec<u8>) {
        if let Some(u) = self.online.get(user) {
            let _ = u.tx.send(m);
        }
    }

    fn status_of(&self, user: &str) -> u32 {
        self.online.get(user).map(|u| u.status).unwrap_or(0)
    }

    fn broadcast_status(&self, user: &str) {
        let m = msg(GET_USER_STATUS, |w| {
            w.str(user).u32(self.status_of(user)).bool(false);
        });
        for (name, u) in &self.online {
            if name != user {
                let _ = u.tx.send(m.clone());
            }
        }
    }

    fn leave_room(&mut self, room: &str, user: &str) {
        let Some(members) = self.rooms.get_mut(room) else { return };
        if !members.remove(user) {
            return;
        }
        let members: Vec<String> = members.iter().cloned().collect();
        if members.is_empty() {
            self.rooms.remove(room);
        }
        for m in members {
            self.send(&m, msg(USER_LEFT_ROOM, |w| {
                w.str(room).str(user);
            }));
        }
    }

    /// Users sharing at least one liked item with `user`, with the overlap count.
    fn similar(&self, user: &str) -> Vec<(String, u32)> {
        let Some(mine) = self.likes.get(user) else { return vec![] };
        let mut v: Vec<(String, u32)> = self
            .likes
            .iter()
            .filter(|(u, _)| u.as_str() != user)
            .map(|(u, l)| (u.clone(), l.intersection(mine).count() as u32))
            .filter(|(_, n)| *n > 0)
            .collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        v
    }

    fn recommendations(&self, user: &str) -> Vec<(String, i32)> {
        let mine = self.likes.get(user).cloned().unwrap_or_default();
        let mut score: BTreeMap<String, i32> = BTreeMap::new();
        for (other, _) in self.similar(user) {
            for item in self.likes.get(&other).into_iter().flatten() {
                if !mine.contains(item) {
                    *score.entry(item.clone()).or_default() += 1;
                }
            }
        }
        let mut v: Vec<_> = score.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        v
    }

    fn global(&self) -> Vec<(String, i32)> {
        let mut score: BTreeMap<String, i32> = BTreeMap::new();
        for l in self.likes.values() {
            for item in l {
                *score.entry(item.clone()).or_default() += 1;
            }
        }
        for h in self.hates.values() {
            for item in h {
                *score.entry(item.clone()).or_default() -= 1;
            }
        }
        let mut v: Vec<_> = score.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        v
    }
}

fn write_scored(w: &mut Writer, v: &[(String, i32)]) {
    w.u32(v.len() as u32);
    for (s, n) in v {
        w.str(s).i32(*n);
    }
}

async fn read_frame(rd: &mut (impl AsyncReadExt + Unpin)) -> std::io::Result<Vec<u8>> {
    let mut len = [0u8; 4];
    rd.read_exact(&mut len).await?;
    let len = u32::from_le_bytes(len) as usize;
    if len > 16 * 1024 * 1024 {
        return Err(std::io::Error::other("frame too large"));
    }
    let mut buf = vec![0u8; len];
    rd.read_exact(&mut buf).await?;
    Ok(buf)
}

async fn client(state: Shared, stream: TcpStream, addr: SocketAddr) {
    let ip = match addr.ip() {
        std::net::IpAddr::V4(v4) => v4,
        std::net::IpAddr::V6(v6) => v6.to_ipv4_mapped().unwrap_or(Ipv4Addr::LOCALHOST),
    };
    let (mut rd, mut wr) = stream.into_split();
    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let writer = tokio::spawn(async move {
        while let Some(b) = rx.recv().await {
            if wr.write_all(&b).await.is_err() {
                break;
            }
        }
    });
    let mut me: Option<(String, u64)> = None;
    while let Ok(f) = read_frame(&mut rd).await {
        match handle(&state, &tx, ip, &mut me, &f) {
            Ok(true) => {}
            Ok(false) => break,
            Err(e) => eprintln!("mockserver: bad message from {addr}: {e}"),
        }
    }
    if let Some((name, conn)) = me {
        let mut s = state.lock().unwrap();
        if s.online.get(&name).is_some_and(|u| u.conn == conn) {
            s.online.remove(&name);
            let rooms: Vec<String> = s.rooms.iter().filter(|(_, m)| m.contains(&name)).map(|(r, _)| r.clone()).collect();
            for r in rooms {
                s.leave_room(&r, &name);
            }
            s.broadcast_status(&name);
            println!("mockserver: {name} logged out");
        }
    }
    // let queued frames (e.g. a login failure) drain before closing
    drop(tx);
    let _ = writer.await;
}

/// Returns Ok(false) to close the connection.
fn handle(state: &Shared, tx: &Tx, ip: Ipv4Addr, me: &mut Option<(String, u64)>, f: &[u8]) -> anyhow::Result<bool> {
    let mut r = Reader::new(f);
    let code = r.u32()?;
    let mut s = state.lock().unwrap();
    if code == LOGIN {
        let username = r.str()?;
        let password = r.str()?;
        if username.is_empty() {
            let _ = tx.send(msg(LOGIN, |w| {
                w.bool(false).str("INVALIDUSERNAME");
            }));
            return Ok(false);
        }
        match s.passwords.get(&username) {
            Some(p) if p != &password => {
                let _ = tx.send(msg(LOGIN, |w| {
                    w.bool(false).str("INVALIDPASS");
                }));
                return Ok(false);
            }
            Some(_) => {}
            None => {
                s.passwords.insert(username.clone(), password);
            }
        }
        if let Some(old) = s.online.remove(&username) {
            let _ = old.tx.send(msg(RELOGGED, |_| {}));
        }
        s.next_conn += 1;
        let conn = s.next_conn;
        s.online.insert(username.clone(), User { conn, tx: tx.clone(), ip, port: 0, status: 2, files: 0, dirs: 0 });
        *me = Some((username.clone(), conn));
        let _ = tx.send(msg(LOGIN, |w| {
            w.bool(true).str("Welcome to the cseek mock server").ip(ip).str("").bool(false);
        }));
        let _ = tx.send(msg(PRIVILEGED_USERS, |w| {
            w.u32(0);
        }));
        let _ = tx.send(msg(PARENT_MIN_SPEED, |w| {
            w.u32(1);
        }));
        let _ = tx.send(msg(PARENT_SPEED_RATIO, |w| {
            w.u32(50);
        }));
        let _ = tx.send(msg(WISHLIST_INTERVAL, |w| {
            w.u32(720);
        }));
        for (id, ts, from, text) in s.pending_messages.remove(&username).unwrap_or_default() {
            let _ = tx.send(msg(MESSAGE_USER, |w| {
                w.u32(id).u32(ts).str(&from).str(&text).bool(false);
            }));
        }
        s.broadcast_status(&username);
        println!("mockserver: {username} logged in from {ip}");
        return Ok(true);
    }
    let Some((me_name, _)) = me.clone() else {
        // everything else needs a login
        return Ok(false);
    };
    match code {
        SET_WAIT_PORT => {
            let port = r.u32()?;
            if let Some(u) = s.online.get_mut(&me_name) {
                u.port = port;
            }
        }
        GET_PEER_ADDRESS => {
            let user = r.str()?;
            let (ip, port) = s.online.get(&user).map(|u| (u.ip, u.port)).unwrap_or((Ipv4Addr::UNSPECIFIED, 0));
            let _ = tx.send(msg(GET_PEER_ADDRESS, |w| {
                w.str(&user).ip(ip).u32(port).u32(0).u16(0);
            }));
        }
        WATCH_USER => {
            let user = r.str()?;
            let u = s.online.get(&user);
            let known = u.is_some() || s.passwords.contains_key(&user);
            let _ = tx.send(msg(WATCH_USER, |w| {
                w.str(&user).bool(known);
                if known {
                    w.u32(u.map(|u| u.status).unwrap_or(0));
                    stats(w, u);
                    if u.is_some_and(|u| u.status == 0) || u.is_none() {
                        // offline users have no country field
                    } else {
                        w.str("");
                    }
                }
            }));
        }
        UNWATCH_USER => {}
        GET_USER_STATUS => {
            let user = r.str()?;
            let st = s.status_of(&user);
            let _ = tx.send(msg(GET_USER_STATUS, |w| {
                w.str(&user).u32(st).bool(false);
            }));
        }
        GET_USER_STATS => {
            let user = r.str()?;
            let u = s.online.get(&user);
            let _ = tx.send(msg(GET_USER_STATS, |w| {
                w.str(&user);
                stats(w, u);
            }));
        }
        SET_STATUS => {
            let st = r.u32()?;
            if let Some(u) = s.online.get_mut(&me_name) {
                u.status = st;
            }
            s.broadcast_status(&me_name);
        }
        SHARED_FOLDERS_FILES => {
            let dirs = r.u32()?;
            let files = r.u32()?;
            if let Some(u) = s.online.get_mut(&me_name) {
                u.dirs = dirs;
                u.files = files;
            }
        }
        CONNECT_TO_PEER => {
            let token = r.u32()?;
            let user = r.str()?;
            let kind = r.str()?;
            let me_u = s.online.get(&me_name).map(|u| (u.ip, u.port));
            if let (Some((mip, mport)), true) = (me_u, s.online.contains_key(&user)) {
                s.send(&user, msg(CONNECT_TO_PEER, |w| {
                    w.str(&me_name).str(&kind).ip(mip).u32(mport).u32(token).bool(false);
                }));
            } else {
                let _ = tx.send(msg(CANT_CONNECT_TO_PEER, |w| {
                    w.u32(token);
                }));
            }
        }
        MESSAGE_USER => {
            let user = r.str()?;
            let text = r.str()?;
            s.next_msg += 1;
            let id = s.next_msg;
            let ts = now();
            if s.online.contains_key(&user) {
                s.send(&user, msg(MESSAGE_USER, |w| {
                    w.u32(id).u32(ts).str(&me_name).str(&text).bool(true);
                }));
            } else if s.passwords.contains_key(&user) {
                s.pending_messages.entry(user).or_default().push((id, ts, me_name.clone(), text));
            }
        }
        MESSAGE_ACKED | SERVER_PING | HAVE_NO_PARENT | BRANCH_LEVEL | BRANCH_ROOT | ACCEPT_CHILDREN | CHILD_DEPTH
        | SEND_UPLOAD_SPEED | 1003 => {}
        FILE_SEARCH | WISHLIST_SEARCH => {
            let token = r.u32()?;
            let query = r.str()?;
            let m = msg(FILE_SEARCH, |w| {
                w.str(&me_name).u32(token).str(&query);
            });
            for (name, u) in &s.online {
                if name != &me_name {
                    let _ = u.tx.send(m.clone());
                }
            }
        }
        USER_SEARCH => {
            let user = r.str()?;
            let token = r.u32()?;
            let query = r.str()?;
            s.send(&user, msg(FILE_SEARCH, |w| {
                w.str(&me_name).u32(token).str(&query);
            }));
        }
        ROOM_SEARCH => {
            let room = r.str()?;
            let token = r.u32()?;
            let query = r.str()?;
            let members: Vec<String> = s.rooms.get(&room).into_iter().flatten().filter(|m| **m != me_name).cloned().collect();
            for m in members {
                s.send(&m, msg(FILE_SEARCH, |w| {
                    w.str(&me_name).u32(token).str(&query);
                }));
            }
        }
        JOIN_ROOM => {
            let room = r.str()?;
            let members = s.rooms.entry(room.clone()).or_default();
            let fresh = members.insert(me_name.clone());
            let names: Vec<String> = members.iter().cloned().collect();
            let others: Vec<String> = names.iter().filter(|n| **n != me_name).cloned().collect();
            let _ = tx.send(msg(JOIN_ROOM, |w| {
                w.str(&room).strs(&names);
                w.u32(names.len() as u32);
                for n in &names {
                    w.u32(s.status_of(n).max(1));
                }
                w.u32(names.len() as u32);
                for n in &names {
                    stats(w, s.online.get(n));
                }
                w.u32(names.len() as u32);
                for _ in &names {
                    w.u32(0);
                }
                w.strs(&names.iter().map(|_| "").collect::<Vec<_>>());
            }));
            let _ = tx.send(msg(ROOM_TICKER_STATE, |w| {
                w.str(&room).u32(0);
            }));
            if fresh {
                let me_u = s.online.get(&me_name);
                let m = msg(USER_JOINED_ROOM, |w| {
                    w.str(&room).str(&me_name).u32(me_u.map(|u| u.status).unwrap_or(2));
                    stats(w, me_u);
                    w.u32(0).str("");
                });
                for o in others {
                    s.send(&o, m.clone());
                }
            }
        }
        LEAVE_ROOM => {
            let room = r.str()?;
            s.leave_room(&room, &me_name);
            let _ = tx.send(msg(LEAVE_ROOM, |w| {
                w.str(&room);
            }));
        }
        SAY_CHATROOM => {
            let room = r.str()?;
            let text = r.str()?;
            let members: Vec<String> = s.rooms.get(&room).into_iter().flatten().cloned().collect();
            if members.contains(&me_name) {
                let m = msg(SAY_CHATROOM, |w| {
                    w.str(&room).str(&me_name).str(&text);
                });
                for u in members {
                    s.send(&u, m.clone());
                }
            }
        }
        ROOM_LIST => {
            let names: Vec<&String> = s.rooms.keys().collect();
            let counts: Vec<u32> = s.rooms.values().map(|m| m.len() as u32).collect();
            let _ = tx.send(msg(ROOM_LIST, |w| {
                w.strs(&names);
                w.u32(counts.len() as u32);
                for c in counts {
                    w.u32(c);
                }
            }));
        }
        ROOM_TICKER_SET => {
            let room = r.str()?;
            let ticker = r.str()?;
            let members: Vec<String> = s.rooms.get(&room).into_iter().flatten().cloned().collect();
            let m = msg(ROOM_TICKER_ADD, |w| {
                w.str(&room).str(&me_name).str(&ticker);
            });
            for u in members {
                s.send(&u, m.clone());
            }
        }
        CHECK_PRIVILEGES => {
            let _ = tx.send(msg(CHECK_PRIVILEGES, |w| {
                w.u32(0);
            }));
        }
        ADD_THING_I_LIKE => {
            let item = r.str()?;
            s.likes.entry(me_name).or_default().insert(item);
        }
        REMOVE_THING_I_LIKE => {
            let item = r.str()?;
            s.likes.entry(me_name).or_default().remove(&item);
        }
        ADD_THING_I_HATE => {
            let item = r.str()?;
            s.hates.entry(me_name).or_default().insert(item);
        }
        REMOVE_THING_I_HATE => {
            let item = r.str()?;
            s.hates.entry(me_name).or_default().remove(&item);
        }
        USER_INTERESTS => {
            let user = r.str()?;
            let likes: Vec<String> = s.likes.get(&user).into_iter().flatten().cloned().collect();
            let hates: Vec<String> = s.hates.get(&user).into_iter().flatten().cloned().collect();
            let _ = tx.send(msg(USER_INTERESTS, |w| {
                w.str(&user).strs(&likes).strs(&hates);
            }));
        }
        RECOMMENDATIONS => {
            let v = s.recommendations(&me_name);
            let _ = tx.send(msg(RECOMMENDATIONS, |w| {
                write_scored(w, &v);
                w.u32(0);
            }));
        }
        GLOBAL_RECOMMENDATIONS => {
            let v = s.global();
            let _ = tx.send(msg(GLOBAL_RECOMMENDATIONS, |w| {
                write_scored(w, &v);
                w.u32(0);
            }));
        }
        SIMILAR_USERS => {
            let v = s.similar(&me_name);
            let _ = tx.send(msg(SIMILAR_USERS, |w| {
                w.u32(v.len() as u32);
                for (u, n) in &v {
                    w.str(u).u32(*n);
                }
            }));
        }
        ITEM_RECOMMENDATIONS => {
            let item = r.str()?;
            let mut score: BTreeMap<String, i32> = BTreeMap::new();
            for l in s.likes.values().filter(|l| l.contains(&item)) {
                for i in l.iter().filter(|i| **i != item) {
                    *score.entry(i.clone()).or_default() += 1;
                }
            }
            let v: Vec<_> = score.into_iter().collect();
            let _ = tx.send(msg(ITEM_RECOMMENDATIONS, |w| {
                w.str(&item);
                write_scored(w, &v);
            }));
        }
        ITEM_SIMILAR_USERS => {
            let item = r.str()?;
            let users: Vec<String> =
                s.likes.iter().filter(|(u, l)| **u != me_name && l.contains(&item)).map(|(u, _)| u.clone()).collect();
            let _ = tx.send(msg(ITEM_SIMILAR_USERS, |w| {
                w.str(&item).strs(&users);
            }));
        }
        other => println!("mockserver: ignoring code {other} from {me_name}"),
    }
    Ok(true)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut port = 2242u16;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--port" | "-p" => port = args.next().and_then(|p| p.parse().ok()).ok_or_else(|| anyhow::anyhow!("--port needs a number"))?,
            "--help" | "-h" => {
                println!("cseek-mockserver [--port 2242]");
                return Ok(());
            }
            _ => anyhow::bail!("unknown argument {a}"),
        }
    }
    let listener = TcpListener::bind(("127.0.0.1", port)).await?;
    println!("mockserver: listening on 127.0.0.1:{port}");
    let state: Shared = Arc::default();
    loop {
        let (stream, addr) = listener.accept().await?;
        let _ = stream.set_nodelay(true);
        tokio::spawn(client(state.clone(), stream, addr));
    }
}
