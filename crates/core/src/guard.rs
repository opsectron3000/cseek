//! Network guard: per-user rate limits, violation tracking and temporary bans.

use crate::client::Client;
use crate::db::Db;
use crate::model::*;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Default)]
pub(crate) struct Guard {
    enabled: AtomicBool,
    /// (username, kind) → recent event times within the last minute
    rates: Mutex<HashMap<(String, String), Vec<Instant>>>,
    violations: Mutex<HashMap<(String, String), Violation>>,
    bans: Mutex<HashMap<String, Ban>>,
}

impl Guard {
    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::Relaxed);
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn load(&self, db: &Db) {
        let bans: Vec<Ban> = db.all("ban");
        *self.bans.lock() = bans.into_iter().map(|b| (b.username.clone(), b)).collect();
        let v: Vec<Violation> = db.all("violation");
        *self.violations.lock() = v.into_iter().map(|v| ((v.username.clone(), v.kind.clone()), v)).collect();
    }

    pub fn is_banned(&self, username: &str) -> bool {
        let bans = self.bans.lock();
        match bans.get(username) {
            Some(b) => b.until.is_none_or(|u| u > chrono::Utc::now()),
            None => false,
        }
    }

    /// Sliding one-minute rate limit. Always allows when the guard is off or `per_minute` is 0.
    pub fn allow_rate(&self, username: &str, kind: &str, per_minute: u32) -> bool {
        if !self.enabled() || per_minute == 0 {
            return true;
        }
        let mut rates = self.rates.lock();
        let v = rates.entry((username.to_owned(), kind.to_owned())).or_default();
        let now = Instant::now();
        v.retain(|t| now.duration_since(*t) < Duration::from_secs(60));
        if v.len() >= per_minute as usize {
            return false;
        }
        v.push(now);
        true
    }

    /// Record a violation; returns the user's total violation count.
    fn add_violation(&self, db: &Db, username: &str, kind: &str) -> u64 {
        let mut vs = self.violations.lock();
        let v = vs.entry((username.to_owned(), kind.to_owned())).or_insert_with(|| Violation {
            username: username.to_owned(),
            kind: kind.to_owned(),
            count: 0,
            last_at: chrono::Utc::now(),
        });
        v.count += 1;
        v.last_at = chrono::Utc::now();
        db.put("violation", &format!("{username}\u{0}{kind}"), v);
        vs.values().filter(|v| v.username == username).map(|v| v.count).sum()
    }

    fn ban(&self, db: &Db, username: &str, reason: &str, minutes: u64) {
        let until = (minutes > 0).then(|| chrono::Utc::now() + chrono::Duration::minutes(minutes as i64));
        let b = Ban { username: username.to_owned(), reason: reason.to_owned(), until };
        db.put("ban", username, &b);
        self.bans.lock().insert(username.to_owned(), b);
    }

    pub fn unban(&self, db: &Db, username: &str) -> bool {
        db.delete("ban", username);
        // forgive past violations so the next one does not immediately re-ban
        let mut vs = self.violations.lock();
        let keys: Vec<_> = vs.keys().filter(|k| k.0 == username).cloned().collect();
        for k in keys {
            db.delete("violation", &format!("{}\u{0}{}", k.0, k.1));
            vs.remove(&k);
        }
        self.bans.lock().remove(username).is_some()
    }

    /// Drop expired bans, stale rate windows and violations older than a day.
    pub fn expire(&self, db: &Db) {
        let now = chrono::Utc::now();
        let mut bans = self.bans.lock();
        let expired: Vec<String> =
            bans.values().filter(|b| b.until.is_some_and(|u| u <= now)).map(|b| b.username.clone()).collect();
        for u in expired {
            bans.remove(&u);
            db.delete("ban", &u);
        }
        drop(bans);
        let mut vs = self.violations.lock();
        let old: Vec<_> = vs.iter().filter(|(_, v)| now - v.last_at > chrono::Duration::days(1)).map(|(k, _)| k.clone()).collect();
        for k in old {
            db.delete("violation", &format!("{}\u{0}{}", k.0, k.1));
            vs.remove(&k);
        }
        drop(vs);
        let n = Instant::now();
        self.rates.lock().retain(|_, v| v.last().is_some_and(|t| n.duration_since(*t) < Duration::from_secs(60)));
    }

    pub fn state(&self) -> GuardState {
        let mut violations: Vec<Violation> = self.violations.lock().values().cloned().collect();
        violations.sort_by(|a, b| b.last_at.cmp(&a.last_at));
        let mut bans: Vec<Ban> = self.bans.lock().values().cloned().collect();
        bans.sort_by(|a, b| a.username.cmp(&b.username));
        GuardState { violations, bans }
    }
}

impl Client {
    /// Record misbehaviour; bans the user once they reach the configured threshold.
    pub(crate) fn record_violation(self: &Arc<Self>, username: &str, kind: &str) {
        if !self.guard.enabled() {
            return;
        }
        let total = self.guard.add_violation(&self.db, username, kind);
        self.record_reputation(username, |r| r.violations += 1);
        tracing::warn!("guard: {username} violation {kind} (total {total})");
        let (threshold, minutes) = {
            let c = self.config.read();
            (c.guard.ban_threshold, c.guard.ban_minutes)
        };
        if threshold > 0 && total >= threshold as u64 && !self.guard.is_banned(username) {
            tracing::warn!("guard: banning {username} for {minutes} minutes");
            self.guard.ban(&self.db, username, &format!("{total} violations ({kind})"), minutes);
            self.cancel_user_transfers(username, "Banned by network guard");
        }
    }

    pub fn guard_state(&self) -> GuardState {
        self.guard.state()
    }

    pub fn unban(&self, username: &str) -> bool {
        self.guard.unban(&self.db, username)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_and_bans() {
        let db = Db::in_memory().unwrap();
        let g = Guard::default();
        assert!(g.allow_rate("a", "k", 1));
        assert!(g.allow_rate("a", "k", 1), "disabled guard never limits");
        g.set_enabled(true);
        assert!(g.allow_rate("b", "k", 2));
        assert!(g.allow_rate("b", "k", 2));
        assert!(!g.allow_rate("b", "k", 2));
        assert_eq!(g.add_violation(&db, "b", "x"), 1);
        assert_eq!(g.add_violation(&db, "b", "y"), 2);
        g.ban(&db, "b", "test", 10);
        assert!(g.is_banned("b"));
        let g2 = Guard::default();
        g2.load(&db);
        assert!(g2.is_banned("b"));
        assert!(g2.unban(&db, "b"));
        assert!(!g2.is_banned("b"));
        assert!(g2.state().violations.is_empty());
    }
}
