//! Web authentication: session tokens (persisted, 30 days), API keys, and
//! `?token=` for endpoints the browser loads directly (media, pictures, WS, webhooks).

use crate::api::{ApiError, Ctx};
use axum::extract::{OriginalUri, Request, State};
use axum::http::{header, Method};
use axum::middleware::Next;
use axum::response::Response;
use serde::{Deserialize, Serialize};

const SESSION_KIND: &str = "web_session";
const SESSION_DAYS: i64 = 30;

#[derive(Serialize, Deserialize)]
struct WebSession {
    username: String,
    expires: chrono::DateTime<chrono::Utc>,
}

/// Compare without leaking the position of the first mismatch.
pub fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut diff = (a.len() != b.len()) as u8;
    for i in 0..a.len().max(b.len()) {
        diff |= a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0);
    }
    diff == 0
}

pub fn create_session(ctx: &Ctx, username: &str) -> String {
    let token: String = (0..32).map(|_| format!("{:02x}", rand::random::<u8>())).collect();
    ctx.client.db.put(
        SESSION_KIND,
        &token,
        &WebSession { username: username.to_owned(), expires: chrono::Utc::now() + chrono::Duration::days(SESSION_DAYS) },
    );
    token
}

pub fn delete_session(ctx: &Ctx, token: &str) {
    ctx.client.db.delete(SESSION_KIND, token);
}

/// Resolve a credential (session token or API key) to a username.
pub fn check_credential(ctx: &Ctx, cred: &str) -> Option<String> {
    if cred.is_empty() {
        return None;
    }
    let cfg = ctx.client.config();
    if cfg.web.auth.api_keys.iter().any(|k| !k.is_empty() && ct_eq(k, cred)) {
        return Some("api-key".into());
    }
    if cred.len() != 64 || !cred.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let s: WebSession = ctx.client.db.get(SESSION_KIND, cred)?;
    if s.expires < chrono::Utc::now() {
        delete_session(ctx, cred);
        return None;
    }
    Some(s.username)
}

/// Endpoints where `?token=` is accepted because the caller cannot set headers.
fn query_token_allowed(method: &Method, path: &str) -> bool {
    let get = method == Method::GET || method == Method::HEAD;
    (get && (path == "/api/events" || path.starts_with("/api/stream/") || (path.starts_with("/api/users/") && path.ends_with("/picture"))))
        || (method == Method::POST && path.starts_with("/api/nowplaying/webhook/"))
}

pub fn request_credential(req: &Request) -> Option<String> {
    let h = req.headers();
    if let Some(v) = h.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) {
        if let Some(t) = v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer ")) {
            return Some(t.trim().to_owned());
        }
    }
    if let Some(v) = h.get("x-api-key").and_then(|v| v.to_str().ok()) {
        return Some(v.trim().to_owned());
    }
    // inside `nest("/api")` the request URI has the prefix stripped; match on the original
    let uri = req.extensions().get::<OriginalUri>().map(|u| &u.0).unwrap_or(req.uri());
    if query_token_allowed(req.method(), uri.path()) {
        let q = uri.query().unwrap_or_default();
        for pair in q.split('&') {
            if let Some(v) = pair.strip_prefix("token=").or_else(|| pair.strip_prefix("apiKey=")) {
                return Some(percent_decode(v));
            }
        }
    }
    None
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub async fn require_auth(State(ctx): State<Ctx>, req: Request, next: Next) -> Result<Response, ApiError> {
    if ctx.client.config().web.auth.disabled {
        return Ok(next.run(req).await);
    }
    match request_credential(&req).and_then(|c| check_credential(&ctx, &c)) {
        Some(_) => Ok(next.run(req).await),
        None => Err(ApiError::unauthorized()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert!(ct_eq("abc", "abc"));
        assert!(!ct_eq("abc", "abd"));
        assert!(!ct_eq("abc", "abcd"));
        assert_eq!(percent_decode("a%20b+c"), "a b c");
        assert_eq!(percent_decode("100%"), "100%");
        assert!(query_token_allowed(&Method::GET, "/api/stream/file"));
        assert!(!query_token_allowed(&Method::GET, "/api/options"));
    }
}
