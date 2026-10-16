//! Socket helpers: frame reading with limits, rate limiting.

use parking_lot::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt};

/// Read one length-prefixed frame (payload only). Errors on EOF or oversize frames.
pub async fn read_frame<R: AsyncRead + Unpin>(r: &mut R, max: usize) -> std::io::Result<Vec<u8>> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len).await?;
    let len = u32::from_le_bytes(len) as usize;
    if len > max {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("frame too large: {len}")));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await?;
    Ok(buf)
}

pub async fn read_frame_timeout<R: AsyncRead + Unpin>(r: &mut R, max: usize, t: Duration) -> std::io::Result<Vec<u8>> {
    match tokio::time::timeout(t, read_frame(r, max)).await {
        Ok(r) => r,
        Err(_) => Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "timed out waiting for frame")),
    }
}

/// Token-bucket rate limiter; a rate of 0 means unlimited. Rate can change at runtime.
pub struct Limiter {
    rate: AtomicU64,
    state: Mutex<(f64, Instant)>,
}

impl Limiter {
    pub fn new(bytes_per_sec: u64) -> Self {
        Self { rate: AtomicU64::new(bytes_per_sec), state: Mutex::new((0.0, Instant::now())) }
    }

    pub fn set_rate(&self, bytes_per_sec: u64) {
        self.rate.store(bytes_per_sec, Ordering::Relaxed);
    }

    pub fn rate(&self) -> u64 {
        self.rate.load(Ordering::Relaxed)
    }

    /// Wait until `n` bytes may be transferred.
    pub async fn acquire(&self, n: usize) {
        loop {
            let rate = self.rate();
            if rate == 0 {
                return;
            }
            let wait = {
                let mut s = self.state.lock();
                let now = Instant::now();
                let elapsed = now.duration_since(s.1).as_secs_f64();
                // allow at most ~1/4 second of burst
                s.0 = (s.0 + elapsed * rate as f64).min(rate as f64 / 4.0 + n as f64);
                s.1 = now;
                if s.0 >= n as f64 {
                    s.0 -= n as f64;
                    return;
                }
                Duration::from_secs_f64((n as f64 - s.0) / rate as f64)
            };
            tokio::time::sleep(wait.min(Duration::from_millis(250))).await;
        }
    }
}

/// Rolling byte-rate meter.
pub struct RateMeter {
    total: AtomicU64,
    last: Mutex<(u64, Instant, f64)>,
}

impl Default for RateMeter {
    fn default() -> Self {
        Self { total: AtomicU64::new(0), last: Mutex::new((0, Instant::now(), 0.0)) }
    }
}

impl RateMeter {
    pub fn add(&self, n: u64) {
        self.total.fetch_add(n, Ordering::Relaxed);
    }
    pub fn total(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }
    /// Bytes/second, smoothed; call periodically.
    pub fn sample(&self) -> f64 {
        let mut l = self.last.lock();
        let now = Instant::now();
        let dt = now.duration_since(l.1).as_secs_f64();
        if dt >= 0.5 {
            let total = self.total();
            let inst = (total - l.0) as f64 / dt;
            l.2 = if l.2 == 0.0 { inst } else { l.2 * 0.5 + inst * 0.5 };
            if inst == 0.0 && l.2 < 1.0 {
                l.2 = 0.0;
            }
            l.0 = total;
            l.1 = now;
        }
        l.2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn limiter_paces() {
        let l = Limiter::new(100_000);
        let start = Instant::now();
        for _ in 0..10 {
            l.acquire(25_000).await;
        }
        // 250 KB at 100 KB/s with a 25 KB burst allowance ≈ 2.25 s
        assert!(start.elapsed() >= Duration::from_millis(1900), "{:?}", start.elapsed());
    }

    #[tokio::test]
    async fn unlimited_is_instant() {
        let l = Limiter::new(0);
        let start = Instant::now();
        l.acquire(10_000_000).await;
        assert!(start.elapsed() < Duration::from_millis(50));
    }
}
