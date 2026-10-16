import type { SearchFile, SearchResponse } from '../api/types';
import { LOSSLESS, basename, dirname, isAudioExt } from './paths';

/**
 * slskd-compatible result filter syntax.
 *
 *   minbr:320 | minbitrate:320   minimum bitrate (kbps)
 *   minsize:10                    minimum file size (MB)
 *   maxsize:500                   maximum file size (MB)
 *   minlen:180 | minlength:180    minimum length (seconds)
 *   minfif:8 | minfilesinfolder:8 minimum matching files per folder
 *   isvbr | iscbr                 VBR / CBR only
 *   islossless | islossy          lossless / lossy audio only
 *   isfree                        users with a free upload slot only
 *   -word                         exclude paths containing "word"
 *   word                          include only paths containing "word"
 */
export interface ParsedFilter {
  minBitRate: number | null;
  minSize: number | null; // bytes
  maxSize: number | null; // bytes
  minLength: number | null; // seconds
  minFilesInFolder: number | null;
  isVbr: boolean;
  isCbr: boolean;
  isLossless: boolean;
  isLossy: boolean;
  isFree: boolean;
  include: string[];
  exclude: string[];
}

const MB = 1024 * 1024;

export function parseFilter(input: string): ParsedFilter {
  const f: ParsedFilter = {
    minBitRate: null,
    minSize: null,
    maxSize: null,
    minLength: null,
    minFilesInFolder: null,
    isVbr: false,
    isCbr: false,
    isLossless: false,
    isLossy: false,
    isFree: false,
    include: [],
    exclude: [],
  };
  for (const raw of input.trim().split(/\s+/)) {
    if (!raw) continue;
    const token = raw.toLowerCase();
    const m = /^([a-z]+):(\d+(?:\.\d+)?)$/.exec(token);
    if (m) {
      const n = Number(m[2]);
      switch (m[1]) {
        case 'minbr':
        case 'minbitrate':
          f.minBitRate = n;
          continue;
        case 'minsize':
          f.minSize = n * MB;
          continue;
        case 'maxsize':
          f.maxSize = n * MB;
          continue;
        case 'minlen':
        case 'minlength':
          f.minLength = n;
          continue;
        case 'minfif':
        case 'minfilesinfolder':
          f.minFilesInFolder = n;
          continue;
      }
    }
    switch (token) {
      case 'isvbr':
        f.isVbr = true;
        continue;
      case 'iscbr':
        f.isCbr = true;
        continue;
      case 'islossless':
        f.isLossless = true;
        continue;
      case 'islossy':
        f.isLossy = true;
        continue;
      case 'isfree':
        f.isFree = true;
        continue;
    }
    if (token.startsWith('-') && token.length > 1) f.exclude.push(token.slice(1));
    else f.include.push(token);
  }
  return f;
}

export function isEmptyFilter(f: ParsedFilter): boolean {
  return (
    f.minBitRate === null &&
    f.minSize === null &&
    f.maxSize === null &&
    f.minLength === null &&
    f.minFilesInFolder === null &&
    !f.isVbr &&
    !f.isCbr &&
    !f.isLossless &&
    !f.isLossy &&
    !f.isFree &&
    f.include.length === 0 &&
    f.exclude.length === 0
  );
}

function fileExt(file: SearchFile): string {
  return (file.extension || basename(file.filename).split('.').pop() || '').toLowerCase();
}

export function fileMatches(file: SearchFile, f: ParsedFilter): boolean {
  const ext = fileExt(file);
  if (f.minBitRate !== null) {
    // Lossless files without bitrate info pass a bitrate floor.
    if (file.bitRate === undefined ? !LOSSLESS.has(ext) : file.bitRate < f.minBitRate) return false;
  }
  if (f.minSize !== null && file.size < f.minSize) return false;
  if (f.maxSize !== null && file.size > f.maxSize) return false;
  if (f.minLength !== null && (file.length === undefined || file.length < f.minLength)) return false;
  if (f.isVbr && file.isVariableBitRate !== true) return false;
  if (f.isCbr && file.isVariableBitRate !== false) return false;
  if (f.isLossless && !LOSSLESS.has(ext)) return false;
  if (f.isLossy && (LOSSLESS.has(ext) || !isAudioExt(ext))) return false;
  if (f.include.length || f.exclude.length) {
    const hay = file.filename.toLowerCase();
    for (const w of f.exclude) if (hay.includes(w)) return false;
    for (const w of f.include) if (!hay.includes(w)) return false;
  }
  return true;
}

export interface FolderGroup {
  path: string;
  files: SearchFile[];
  locked: boolean;
}

export interface UserGroup {
  response: SearchResponse;
  folders: FolderGroup[];
  fileCount: number;
}

/** Filter a response set and group by user, then folder. Preserves server score order. */
export function groupResponses(
  responses: SearchResponse[],
  f: ParsedFilter,
  opts: { hideUsers?: Set<string>; showLocked?: boolean } = {},
): UserGroup[] {
  const out: UserGroup[] = [];
  const sorted = [...responses].sort((a, b) => b.score - a.score);
  for (const r of sorted) {
    if (opts.hideUsers?.has(r.username)) continue;
    if (f.isFree && !r.hasFreeUploadSlot) continue;
    const folders = new Map<string, FolderGroup>();
    const add = (file: SearchFile, locked: boolean) => {
      if (!fileMatches(file, f)) return;
      const dir = dirname(file.filename);
      const key = `${locked ? 'L' : 'U'}${dir}`;
      let g = folders.get(key);
      if (!g) {
        g = { path: dir, files: [], locked };
        folders.set(key, g);
      }
      g.files.push(file);
    };
    r.files.forEach((file) => add(file, false));
    if (opts.showLocked) r.lockedFiles.forEach((file) => add(file, true));
    let list = [...folders.values()];
    if (f.minFilesInFolder !== null) list = list.filter((g) => g.files.length >= f.minFilesInFolder!);
    if (!list.length) continue;
    list.forEach((g) => g.files.sort((a, b) => a.filename.localeCompare(b.filename, undefined, { numeric: true })));
    list.sort((a, b) => Number(a.locked) - Number(b.locked) || a.path.localeCompare(b.path));
    out.push({ response: r, folders: list, fileCount: list.reduce((n, g) => n + g.files.length, 0) });
  }
  return out;
}

/**
 * Alternate sources for multi-source downloads: other users in the same search
 * who have a file with identical size and the same basename (case-insensitive).
 */
export function findAlternates(
  responses: SearchResponse[],
  username: string,
  file: { filename: string; size: number },
  hideUsers?: Set<string>,
): { username: string; filename: string }[] {
  const name = basename(file.filename).toLowerCase();
  const out: { username: string; filename: string }[] = [];
  for (const r of responses) {
    if (r.username === username || hideUsers?.has(r.username)) continue;
    const match = r.files.find((f) => f.size === file.size && basename(f.filename).toLowerCase() === name);
    if (match) out.push({ username: r.username, filename: match.filename });
  }
  return out;
}

export const FILTER_HELP: [string, string][] = [
  ['minbr:320', 'minimum bitrate (kbps)'],
  ['minsize:10', 'minimum size (MB)'],
  ['maxsize:500', 'maximum size (MB)'],
  ['minlen:180', 'minimum length (seconds)'],
  ['minfif:8', 'minimum files per folder'],
  ['isvbr / iscbr', 'variable / constant bitrate'],
  ['islossless / islossy', 'lossless / lossy audio'],
  ['isfree', 'free upload slot only'],
  ['-live', 'exclude paths containing a word'],
  ['flac', 'paths must contain a word'],
];
