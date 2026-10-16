const UNITS = ['B', 'KB', 'MB', 'GB', 'TB'];

export function formatBytes(n: number | null | undefined, digits = 1): string {
  if (n === null || n === undefined || !Number.isFinite(n)) return '—';
  if (n < 1024) return `${Math.round(n)} B`;
  let v = n;
  let i = 0;
  while (v >= 1024 && i < UNITS.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v >= 100 ? 0 : digits)} ${UNITS[i]}`;
}

export function formatSpeed(bytesPerSec: number | null | undefined): string {
  if (!bytesPerSec) return '0 B/s';
  return `${formatBytes(bytesPerSec)}/s`;
}

/** Seconds -> m:ss or h:mm:ss */
export function formatDuration(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined || !Number.isFinite(seconds) || seconds < 0) return '—:——';
  const s = Math.floor(seconds);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = String(s % 60).padStart(2, '0');
  return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${sec}` : `${m}:${sec}`;
}

/** Seconds -> "3d 4h", "5h 12m", "42s" */
export function formatUptime(seconds: number): string {
  const s = Math.floor(seconds);
  const d = Math.floor(s / 86400);
  const h = Math.floor((s % 86400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m ${s % 60}s`;
  return `${s}s`;
}

const rtf = new Intl.RelativeTimeFormat(undefined, { numeric: 'auto' });

export function formatRelative(iso: string | null | undefined, now = Date.now()): string {
  if (!iso) return 'never';
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return '—';
  const diff = (t - now) / 1000;
  const abs = Math.abs(diff);
  if (abs < 45) return diff <= 0 ? 'just now' : 'in a moment';
  if (abs < 3600) return rtf.format(Math.round(diff / 60), 'minute');
  if (abs < 86400) return rtf.format(Math.round(diff / 3600), 'hour');
  if (abs < 86400 * 30) return rtf.format(Math.round(diff / 86400), 'day');
  return new Date(t).toLocaleDateString();
}

export function formatTime(iso: string): string {
  const d = new Date(iso);
  const today = new Date();
  if (d.toDateString() === today.toDateString()) {
    return d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
  }
  return d.toLocaleString(undefined, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
}

export function formatDateTime(iso: string | null | undefined): string {
  if (!iso) return '—';
  return new Date(iso).toLocaleString();
}

export function formatNumber(n: number): string {
  return n.toLocaleString();
}

export function formatPercent(fraction: number): string {
  return `${Math.round(Math.max(0, Math.min(1, fraction)) * 100)}%`;
}

export function plural(n: number, word: string, pluralWord = `${word}s`): string {
  return `${n.toLocaleString()} ${n === 1 ? word : pluralWord}`;
}

/** Short audio quality summary for a file: "320 kbps", "FLAC 24/96", "V0 ~245" */
export function formatQuality(f: { extension: string; bitRate?: number; bitDepth?: number; sampleRate?: number; isVariableBitRate?: boolean }): string {
  const parts: string[] = [];
  if (f.bitDepth && f.sampleRate) parts.push(`${f.bitDepth}/${f.sampleRate >= 1000 ? +(f.sampleRate / 1000).toFixed(1) : f.sampleRate}`);
  else if (f.bitRate) parts.push(`${f.bitRate}${f.isVariableBitRate ? ' vbr' : ''}`);
  return parts.join(' ');
}

const LABEL_OVERRIDES: Record<string, string> = { geoip: 'GeoIP', api: 'API', url: 'URL' };

export function camelToLabel(key: string): string {
  if (LABEL_OVERRIDES[key]) return LABEL_OVERRIDES[key];
  const spaced = key.replace(/([a-z0-9])([A-Z])/g, '$1 $2').replace(/([A-Z]+)([A-Z][a-z])/g, '$1 $2');
  return spaced.charAt(0).toUpperCase() + spaced.slice(1);
}
