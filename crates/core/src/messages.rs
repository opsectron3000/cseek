//! Private messages.

use crate::client::Client;
use crate::model::*;
use anyhow::{bail, Result};
use chrono::TimeZone;
use slsk_proto::ServerRequest;

impl Client {
    pub(crate) fn on_private_message(&self, id: u32, timestamp: u32, username: String, message: String, is_new: bool) {
        if self.is_blacklisted(&username) || self.guard.is_banned(&username) {
            return;
        }
        let ts = chrono::Utc.timestamp_opt(timestamp as i64, 0).single().unwrap_or_else(chrono::Utc::now);
        let m = Message {
            // server ids restart per session; combine with the timestamp for a stable key
            id: format!("{timestamp}-{id}"),
            timestamp: ts,
            username: username.clone(),
            direction: MessageDirection::In,
            message,
        };
        tracing::debug!("message from {username} (new: {is_new})");
        self.db.insert_message(&m, false);
        self.watch_user(&username);
        self.emit(Event::Message(m));
        self.emit_app();
    }

    pub fn send_message(&self, username: &str, message: &str) -> Result<Message> {
        let message = message.trim_end();
        if message.is_empty() {
            bail!("message is empty");
        }
        self.server_send(ServerRequest::MessageUser { username: username.to_owned(), message: message.to_owned() })?;
        let m = Message {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now(),
            username: username.to_owned(),
            direction: MessageDirection::Out,
            message: message.to_owned(),
        };
        self.db.insert_message(&m, true);
        self.watch_user(username);
        self.emit(Event::Message(m.clone()));
        Ok(m)
    }

    pub fn conversations(&self) -> Vec<Conversation> {
        self.db
            .conversations()
            .into_iter()
            .map(|(username, unread, last_message)| Conversation {
                status: self.cached_status(&username),
                username,
                unread,
                last_message,
            })
            .collect()
    }

    pub fn conversation(&self, username: &str) -> Vec<Message> {
        self.db.messages(username, 1000)
    }

    pub fn mark_conversation_read(&self, username: &str) {
        self.db.mark_read(username);
        self.emit_app();
    }

    pub fn delete_conversation(&self, username: &str) {
        self.db.delete_conversation(username);
        self.emit_app();
    }
}
