import type { SearchFile, SearchResponse } from '../api/types';
import type { UserGroup } from './filters';
import { LOSSLESS, basename, isAudioExt, splitRemote } from './paths';

/** One folder from one user: the unit search results are shown in. */
export interface FolderResult {
  key: string;
  response: SearchResponse;
  path: string;
  /** Display title: the folder name, or "Album / CD1" for disc folders. */
  name: string;
  /** A short trail of the folders above it. */
  parent: string;
  files: SearchFile[];
  locked: boolean;
  audioCount: number;
  size: number;
  /** Total audio length in seconds (0 when unknown). */
  duration: number;
  /** Dominant file type, upper case ("FLAC", "MP3"). */
  format: string;
  lossless: boolean;
  /** "24/96", "320", "VBR ~245", or "". */
  quality: string;
  qualityRank: number;
  /** Other users sharing a folder with the same name and track count. */
  alsoFrom: number;
  /** Position among this user's folders, best first. */
  userIndex: number;
  /** Share of query words found in the folder path (0–1). */
  relevance: number;
}

export type SortKey = 'best' | 'quality' | 'tracks' | 'size' | 'speed';

const DISC = /^(cd|disc|disk|dvd|side)[\s._-]*\d+|^\d{1,2}$/i;

function fileExt(f: SearchFile): string {
  return (f.extension || basename(f.filename).split('.').pop() || '').toLowerCase();
}

function titleOf(path: string): { name: string; parent: string } {
  // Drop the "@@share" virtual root that some clients prefix.
  const parts = splitRemote(path).filter((p, i) => !(i === 0 && p.startsWith('@@')));
  if (!parts.length) return { name: path || '(root)', parent: '' };
  let n = 1;
  if (parts.length > 1 && DISC.test(parts[parts.length - 1])) n = 2;
  const name = parts.slice(-n).join(' / ');
  const parent = parts.slice(Math.max(0, parts.length - n - 2), parts.length - n).join(' / ');
  return { name, parent };
}

function mode<T>(values: T[]): T | undefined {
  const counts = new Map<T, number>();
  let best: T | undefined;
  let bestN = 0;
  for (const v of values) {
    const n = (counts.get(v) ?? 0) + 1;
    counts.set(v, n);
    if (n > bestN) {
      best = v;
      bestN = n;
    }
  }
  return best;
}

function khz(sr: number): string {
  return sr >= 1000 ? String(+(sr / 1000).toFixed(1)) : String(sr);
}

function describe(files: SearchFile[]) {
  const audio = files.filter((f) => isAudioExt(fileExt(f)));
  const pool = audio.length ? audio : files;
  const ext = mode(pool.map(fileExt)) ?? '';
  const lossless = LOSSLESS.has(ext);
  const same = pool.filter((f) => fileExt(f) === ext);
  let quality = '';
  let qualityRank = 0;
  if (lossless) {
    const spec = mode(same.filter((f) => f.bitDepth && f.sampleRate).map((f) => `${f.bitDepth}/${khz(f.sampleRate!)}`));
    quality = spec ?? '';
    const [d, s] = (spec ?? '16/44.1').split('/').map(Number);
    qualityRank = 10_000 + d * 100 + s;
  } else {
    const rates = same.map((f) => f.bitRate).filter((b): b is number => !!b);
    if (rates.length) {
      const avg = Math.round(rates.reduce((a, b) => a + b, 0) / rates.length);
      const vbr = same.some((f) => f.isVariableBitRate);
      const uniform = rates.every((r) => r === rates[0]);
      quality = vbr ? `VBR ~${avg}` : uniform ? String(rates[0]) : `~${avg}`;
      qualityRank = avg;
    }
  }
  return {
    audioCount: audio.length,
    duration: audio.reduce((n, f) => n + (f.length ?? 0), 0),
    format: ext.toUpperCase(),
    lossless,
    quality,
    qualityRank,
  };
}

/** Flatten user groups into folder rows, ranked best first. */
export function toFolders(groups: UserGroup[], query = ''): FolderResult[] {
  const words = query.toLowerCase().split(/\s+/).filter((w) => w.length > 1 && !w.startsWith('-'));
  const relevance = (path: string) => {
    if (!words.length) return 0;
    const p = path.toLowerCase().replace(/[_.]+/g, ' ');
    return words.filter((w) => p.includes(w)).length / words.length;
  };
  const out: FolderResult[] = [];
  for (const g of groups) {
    const r = g.response;
    const mine = g.folders.map((f) => {
      const { name, parent } = titleOf(f.path);
      return {
        key: `${r.username}\u0000${f.locked ? 'L' : 'U'}${f.path}`,
        response: r,
        path: f.path,
        name,
        parent,
        files: f.files,
        locked: f.locked,
        size: f.files.reduce((n, x) => n + x.size, 0),
        alsoFrom: 0,
        userIndex: 0,
        relevance: relevance(f.path),
        ...describe(f.files),
      };
    });
    mine.sort((a, b) => Number(a.locked) - Number(b.locked) || b.relevance - a.relevance || b.audioCount - a.audioCount || b.files.length - a.files.length);
    mine.forEach((f, i) => (f.userIndex = i));
    out.push(...mine);
  }

  // Count identical folders (same title and track count) across users.
  const twins = new Map<string, Set<string>>();
  const twinKey = (f: FolderResult) => `${f.name.toLowerCase()}\u0000${f.audioCount || f.files.length}`;
  for (const f of out) {
    const k = twinKey(f);
    let s = twins.get(k);
    if (!s) twins.set(k, (s = new Set()));
    s.add(f.response.username);
  }
  for (const f of out) f.alsoFrom = (twins.get(twinKey(f))?.size ?? 1) - 1;
  return sortFolders(out, 'best');
}

/**
 * "Best" favours folders whose path names what was searched for and that hold
 * a whole release, then interleaves users: a peer's score counts fully for its
 * best folder and a little less for each further one, so one big share can't
 * bury the rest.
 */
function bestRank(f: FolderResult): number {
  const tracks = f.audioCount || f.files.length;
  return (
    f.response.score +
    f.relevance * 40 +
    Math.min(tracks, 10) * 2 +
    Math.min(f.alsoFrom, 10) -
    f.userIndex * 6 -
    (f.locked ? 50 : 0)
  );
}

export function sortFolders(list: FolderResult[], key: SortKey): FolderResult[] {
  const by: Record<SortKey, (f: FolderResult) => number> = {
    best: bestRank,
    quality: (f) => f.qualityRank,
    tracks: (f) => f.audioCount || f.files.length,
    size: (f) => f.size,
    speed: (f) => (f.response.hasFreeUploadSlot ? 1e12 : 0) + f.response.uploadSpeed - f.response.queueLength,
  };
  const rank = by[key];
  return [...list].sort((a, b) => rank(b) - rank(a) || bestRank(b) - bestRank(a));
}
