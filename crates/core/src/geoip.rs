//! User countries: a username → ISO 3166-1 alpha-2 map fed by the server
//! (WatchUser, room user lists) and by GeoIP lookups of peer addresses using
//! the DB-IP Country Lite database (CC BY 4.0, https://db-ip.com).

use crate::client::Client;
use crate::model::*;
use anyhow::{bail, Context, Result};
use chrono::{Datelike, Utc};
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::io::Read;
use std::net::IpAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;

const MAX_ENTRIES: usize = 50_000;
const DB_FILE: &str = "dbip-country-lite.mmdb";
/// DB-IP publishes monthly; refresh once the file is older than this.
const MAX_DB_AGE: Duration = Duration::from_secs(35 * 24 * 3600);
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 3600);

type GeoDb = maxminddb::Reader<Vec<u8>>;

/// Where a country code came from. Server-provided codes win over GeoIP ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum CountrySource {
    GeoIp,
    Server,
}

#[derive(Default)]
pub(crate) struct Countries {
    map: Mutex<HashMap<String, (String, CountrySource)>>,
    db: RwLock<Option<Arc<GeoDb>>>,
    wake: Notify,
}

impl Countries {
    /// Record a country for `username`. Returns the normalised code if it is new or changed.
    pub fn set(&self, username: &str, code: &str, source: CountrySource) -> Option<String> {
        let code = code.trim().to_ascii_uppercase();
        if username.is_empty() || code.len() != 2 || !code.bytes().all(|b| b.is_ascii_uppercase()) {
            return None;
        }
        let mut map = self.map.lock();
        match map.get_mut(username) {
            Some(cur) if cur.1 > source => None,
            Some(cur) if cur.0 == code => {
                cur.1 = source;
                None
            }
            Some(cur) => {
                *cur = (code.clone(), source);
                Some(code)
            }
            None => {
                if map.len() >= MAX_ENTRIES {
                    // evict an arbitrary entry; the map is only a display hint
                    if let Some(k) = map.keys().next().cloned() {
                        map.remove(&k);
                    }
                }
                map.insert(username.to_owned(), (code.clone(), source));
                Some(code)
            }
        }
    }

    pub fn all(&self) -> HashMap<String, String> {
        self.map.lock().iter().map(|(u, (c, _))| (u.clone(), c.clone())).collect()
    }

    fn lookup(&self, ip: IpAddr) -> Option<String> {
        let db = self.db.read().clone()?;
        let c: maxminddb::geoip2::Country = db.lookup(ip).ok()??;
        c.country?.iso_code.map(str::to_owned)
    }
}

/// Only public addresses are worth looking up.
fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_unspecified()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_multicast())
        }
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => is_public(IpAddr::V4(v4)),
            None => {
                let s = v6.segments()[0];
                !(v6.is_loopback() || v6.is_unspecified() || v6.is_multicast() || s & 0xfe00 == 0xfc00 || s & 0xffc0 == 0xfe80)
            }
        },
    }
}

impl Client {
    pub fn user_countries(&self) -> HashMap<String, String> {
        self.countries.all()
    }

    pub(crate) fn set_country(&self, username: &str, code: &str, source: CountrySource) {
        if let Some(country) = self.countries.set(username, code, source) {
            self.emit(Event::UserCountry { username: username.to_owned(), country });
        }
    }

    /// GeoIP-locate a peer from an address we learned for it.
    pub(crate) fn locate_peer(&self, username: &str, ip: IpAddr) {
        if !is_public(ip) {
            return;
        }
        if let Some(code) = self.countries.lookup(ip) {
            self.set_country(username, &code, CountrySource::GeoIp);
        }
    }

    /// Called on config changes: load/refresh when enabled, drop the database when disabled.
    pub(crate) fn geoip_enabled_changed(&self, enabled: bool) {
        if enabled {
            self.countries.wake.notify_one();
        } else {
            *self.countries.db.write() = None;
        }
    }

    /// Keep the GeoIP database present and fresh: on start, daily, and when enabled.
    pub(crate) async fn geoip_loop(self: Arc<Self>) {
        loop {
            if self.config.read().geoip.enabled {
                if let Err(e) = self.refresh_geoip().await {
                    tracing::warn!("geoip: {e:#}");
                }
            }
            tokio::select! {
                _ = tokio::time::sleep(CHECK_INTERVAL) => {}
                _ = self.countries.wake.notified() => {}
            }
        }
    }

    async fn refresh_geoip(&self) -> Result<()> {
        let path = self.base_dir.join("geoip").join(DB_FILE);
        let age = std::fs::metadata(&path).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok());
        if age.is_none_or(|a| a > MAX_DB_AGE) {
            match download_db(&path).await {
                Ok((db, url)) => {
                    tracing::info!("geoip: downloaded {url}");
                    *self.countries.db.write() = Some(Arc::new(db));
                    return Ok(());
                }
                // an outdated database is still better than none
                Err(e) if age.is_some() => tracing::warn!("geoip: update failed, keeping the old database: {e:#}"),
                Err(e) => return Err(e),
            }
        }
        if self.countries.db.read().is_none() {
            let p = path.clone();
            let db = tokio::task::spawn_blocking(move || GeoDb::open_readfile(&p))
                .await?
                .with_context(|| format!("opening {}", path.display()))?;
            *self.countries.db.write() = Some(Arc::new(db));
            tracing::info!("geoip: loaded {}", path.display());
        }
        Ok(())
    }
}

/// Download this month's DB-IP Country Lite (or last month's if not out yet),
/// gunzip it and atomically replace `path`.
async fn download_db(path: &Path) -> Result<(GeoDb, String)> {
    let http = reqwest::Client::builder().timeout(Duration::from_secs(120)).build()?;
    let this_month = Utc::now().date_naive().with_day(1).context("date")?;
    let last_month = (this_month - chrono::Duration::days(1)).with_day(1).context("date")?;
    for (i, month) in [this_month, last_month].into_iter().enumerate() {
        let url = format!(
            "https://download.db-ip.com/free/dbip-country-lite-{}-{:02}.mmdb.gz",
            month.year(),
            month.month()
        );
        let resp = http.get(&url).send().await.with_context(|| format!("downloading {url}"))?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND && i == 0 {
            continue;
        }
        let gz = resp.error_for_status().with_context(|| format!("downloading {url}"))?.bytes().await?;
        let path = path.to_owned();
        let db = tokio::task::spawn_blocking(move || install_db(&gz, &path)).await??;
        return Ok((db, url));
    }
    bail!("no DB-IP database available")
}

fn install_db(gz: &[u8], path: &Path) -> Result<GeoDb> {
    let mut buf = Vec::new();
    flate2::read::GzDecoder::new(gz).read_to_end(&mut buf).context("decompressing database")?;
    // validate before replacing a working file
    maxminddb::Reader::from_source(&buf[..]).context("invalid database")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("mmdb.tmp");
    std::fs::write(&tmp, &buf).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("renaming to {}", path.display()))?;
    Ok(GeoDb::from_source(buf)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_country_beats_geoip() {
        let c = Countries::default();
        assert_eq!(c.set("a", "de", CountrySource::GeoIp).as_deref(), Some("DE"));
        // a server code overwrites GeoIP
        assert_eq!(c.set("a", "FR", CountrySource::Server).as_deref(), Some("FR"));
        // GeoIP never overwrites a server code
        assert_eq!(c.set("a", "US", CountrySource::GeoIp), None);
        assert_eq!(c.all()["a"], "FR");
        // unchanged codes are not reported again
        assert_eq!(c.set("a", "fr", CountrySource::Server), None);
        assert_eq!(c.set("a", "", CountrySource::Server), None);
        assert_eq!(c.set("b", "US", CountrySource::Server).as_deref(), Some("US"));
        assert_eq!(c.set("b", "CA", CountrySource::Server).as_deref(), Some("CA"));
    }

    #[test]
    fn private_addresses_skipped() {
        for ip in ["127.0.0.1", "0.0.0.0", "10.1.2.3", "192.168.1.1", "172.16.0.1", "::1", "fd00::1", "::ffff:10.0.0.1"] {
            assert!(!is_public(ip.parse().unwrap()), "{ip}");
        }
        assert!(is_public("8.8.8.8".parse().unwrap()));
    }
}
