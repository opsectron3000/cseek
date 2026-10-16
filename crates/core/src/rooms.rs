//! Chat rooms.

use crate::client::{Client, Waited};
use crate::geoip::CountrySource;
use crate::model::*;
use anyhow::{anyhow, bail, Result};
use parking_lot::Mutex;
use slsk_proto::server::RoomUserData;
use slsk_proto::{ServerRequest, ServerResponse};
use std::collections::{HashMap, VecDeque};
use std::time::Duration;

/// Messages kept in memory (and sent with a room) per room.
const MAX_ROOM_MESSAGES: usize = 1000;
/// Messages kept on disk per room.
const LOGGED_ROOM_MESSAGES: usize = 20_000;
const SAVED_ROOMS_KEY: &str = "rooms.joined";

struct RoomData {
    is_private: bool,
    users: Vec<RoomUser>,
    messages: VecDeque<RoomMessage>,
    tickers: Vec<Ticker>,
}

#[derive(Default)]
pub(crate) struct RoomsState {
    joined: Mutex<HashMap<String, RoomData>>,
    available: Mutex<Vec<RoomSummary>>,
}

impl RoomsState {
    pub fn joined_names(&self) -> Vec<String> {
        self.joined.lock().keys().cloned().collect()
    }
}

fn room_user(u: &RoomUserData) -> RoomUser {
    RoomUser {
        username: u.username.clone(),
        status: UserStatus::from_code(u.status),
        average_speed: u.stats.avg_speed,
        file_count: u.stats.files,
        country: u.country.clone(),
    }
}

impl Client {
    fn note_room_country(&self, u: &RoomUserData) {
        if let Some(c) = &u.country {
            self.set_country(&u.username, c, CountrySource::Server);
        }
    }

    fn room_snapshot(&self, name: &str, with_messages: bool) -> Option<Room> {
        let joined = self.rooms.joined.lock();
        let r = joined.get(name)?;
        Some(Room {
            name: name.to_owned(),
            is_private: r.is_private,
            users: r.users.clone(),
            messages: if with_messages { r.messages.iter().cloned().collect() } else { vec![] },
            tickers: r.tickers.clone(),
        })
    }

    fn emit_room(&self, name: &str, with_messages: bool) {
        if let Some(r) = self.room_snapshot(name, with_messages) {
            self.emit(Event::RoomUpdated(Box::new(r)));
        }
    }

    /// Rooms to rejoin after login: saved ones plus any joined this session.
    pub(crate) fn saved_rooms(&self) -> Vec<String> {
        let mut v: Vec<String> =
            self.db.kv_get(SAVED_ROOMS_KEY).and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default();
        for n in self.rooms.joined_names() {
            if !v.contains(&n) {
                v.push(n);
            }
        }
        v
    }

    fn save_rooms(&self) {
        let names = self.rooms.joined_names();
        self.db.kv_set(SAVED_ROOMS_KEY, &serde_json::to_string(&names).unwrap_or_default());
    }

    pub(crate) fn handle_room_message(&self, msg: ServerResponse) {
        use ServerResponse as S;
        match msg {
            S::JoinRoom { ref room, ref users, ref owner, .. } => {
                users.iter().for_each(|u| self.note_room_country(u));
                {
                    let mut joined = self.rooms.joined.lock();
                    // History survives restarts: start from the log.
                    let r = joined.entry(room.clone()).or_insert_with(|| RoomData {
                        is_private: false,
                        users: vec![],
                        messages: self.db.room_messages(room, MAX_ROOM_MESSAGES).into(),
                        tickers: vec![],
                    });
                    r.is_private = owner.is_some();
                    r.users = users.iter().map(room_user).collect();
                }
                self.save_rooms();
                self.emit_room(room, true);
                let key = format!("join:{room}");
                self.complete(&key, || Waited::Server(msg.clone()));
            }
            S::LeaveRoom { room } => {
                if self.rooms.joined.lock().remove(&room).is_some() {
                    self.save_rooms();
                    self.emit(Event::RoomLeft { name: room });
                }
            }
            S::UserJoinedRoom { room, user } => {
                self.note_room_country(&user);
                let changed = {
                    let mut joined = self.rooms.joined.lock();
                    match joined.get_mut(&room) {
                        Some(r) => {
                            r.users.retain(|u| u.username != user.username);
                            r.users.push(room_user(&user));
                            true
                        }
                        None => false,
                    }
                };
                if changed {
                    self.emit_room(&room, false);
                }
            }
            S::UserLeftRoom { room, username } => {
                let changed = {
                    let mut joined = self.rooms.joined.lock();
                    match joined.get_mut(&room) {
                        Some(r) => {
                            r.users.retain(|u| u.username != username);
                            true
                        }
                        None => false,
                    }
                };
                if changed {
                    self.emit_room(&room, false);
                }
            }
            S::SayChatroom { room, username, message } => {
                if self.is_blacklisted(&username) {
                    return;
                }
                let m = RoomMessage {
                    timestamp: chrono::Utc::now(),
                    is_self: self.username().as_deref() == Some(username.as_str()),
                    username,
                    message,
                };
                {
                    let mut joined = self.rooms.joined.lock();
                    let Some(r) = joined.get_mut(&room) else { return };
                    self.db.insert_room_message(&room, &m, LOGGED_ROOM_MESSAGES);
                    r.messages.push_back(m.clone());
                    while r.messages.len() > MAX_ROOM_MESSAGES {
                        r.messages.pop_front();
                    }
                }
                self.emit(Event::RoomMessage { room, message: m });
            }
            S::RoomTickerState { room, tickers } => {
                if let Some(r) = self.rooms.joined.lock().get_mut(&room) {
                    r.tickers = tickers.into_iter().map(|(username, message)| Ticker { username, message }).collect();
                }
                self.emit_room(&room, false);
            }
            S::RoomTickerAdd { room, username, ticker } => {
                if let Some(r) = self.rooms.joined.lock().get_mut(&room) {
                    r.tickers.retain(|t| t.username != username);
                    r.tickers.push(Ticker { username, message: ticker });
                }
                self.emit_room(&room, false);
            }
            S::RoomTickerRemove { room, username } => {
                if let Some(r) = self.rooms.joined.lock().get_mut(&room) {
                    r.tickers.retain(|t| t.username != username);
                }
                self.emit_room(&room, false);
            }
            S::RoomList { ref rooms, ref private_rooms } => {
                let mut list: Vec<RoomSummary> = rooms
                    .iter()
                    .map(|(n, c)| RoomSummary { name: n.clone(), user_count: *c, is_private: false })
                    .chain(private_rooms.iter().map(|(n, c)| RoomSummary { name: n.clone(), user_count: *c, is_private: true }))
                    .collect();
                list.sort_by(|a, b| b.user_count.cmp(&a.user_count).then(a.name.cmp(&b.name)));
                *self.rooms.available.lock() = list;
                self.complete("roomlist", || Waited::Server(msg.clone()));
            }
            S::CantCreateRoom { ref room } => {
                let key = format!("join:{room}");
                self.complete(&key, || Waited::Server(msg.clone()));
            }
            _ => {}
        }
    }

    pub async fn available_rooms(&self) -> Result<Vec<RoomSummary>> {
        let cached = self.rooms.available.lock().clone();
        match self.server_request(ServerRequest::RoomList, "roomlist".into(), Duration::from_secs(10)).await {
            Ok(_) => Ok(self.rooms.available.lock().clone()),
            Err(e) if cached.is_empty() => Err(e),
            Err(_) => Ok(cached),
        }
    }

    pub fn joined_rooms(&self) -> Vec<Room> {
        let mut names = self.rooms.joined_names();
        names.sort();
        names.iter().filter_map(|n| self.room_snapshot(n, true)).collect()
    }

    pub fn room(&self, name: &str) -> Option<Room> {
        self.room_snapshot(name, true)
    }

    pub async fn join_room(&self, name: &str) -> Result<Room> {
        let name = name.trim();
        if name.is_empty() {
            bail!("room name is empty");
        }
        if let Some(r) = self.room(name) {
            return Ok(r);
        }
        let r = self
            .server_request(ServerRequest::JoinRoom { room: name.to_owned() }, format!("join:{name}"), Duration::from_secs(15))
            .await?;
        if let ServerResponse::CantCreateRoom { room } = r {
            bail!("cannot create room {room}");
        }
        self.room(name).ok_or_else(|| anyhow!("join failed"))
    }

    pub fn leave_room(&self, name: &str) -> Result<()> {
        let _ = self.server_send(ServerRequest::LeaveRoom { room: name.to_owned() });
        if self.rooms.joined.lock().remove(name).is_some() {
            self.save_rooms();
            self.emit(Event::RoomLeft { name: name.to_owned() });
        }
        Ok(())
    }

    pub fn say_room(&self, name: &str, message: &str) -> Result<()> {
        if !self.rooms.joined.lock().contains_key(name) {
            bail!("not in room {name}");
        }
        let message = message.trim_end();
        if message.is_empty() {
            bail!("message is empty");
        }
        self.server_send(ServerRequest::SayChatroom { room: name.to_owned(), message: message.to_owned() })
    }
}
