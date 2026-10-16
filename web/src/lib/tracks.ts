import { streamFileUrl, streamTransferUrl } from '../api/client';
import type { FileEntry, Transfer } from '../api/types';
import type { Track } from '../store/player';
import { basename, dirname, isPlayable, splitRemote, stripExtension } from './paths';

/** Guess artist/album from a path like "Artist\Album\01 - Title.flac" or "Artist - Album/..." */
function guessMeta(path: string): { title: string; artist?: string; album?: string } {
  const title = stripExtension(basename(path)).replace(/^\d{1,3}[\s.\-_]+/, '');
  const dirs = splitRemote(dirname(path));
  const album = dirs[dirs.length - 1];
  const parent = dirs[dirs.length - 2];
  if (album && album.includes(' - ')) {
    const [artist, ...rest] = album.split(' - ');
    return { title, artist: artist.trim(), album: rest.join(' - ').trim() };
  }
  return { title, artist: parent, album };
}

export function transferTrack(t: Transfer): Track {
  return { key: `t:${t.id}`, src: streamTransferUrl(t.id), ...guessMeta(t.filename) };
}

export function fileTrack(f: FileEntry, root = 'downloads'): Track {
  return { key: `f:${root}:${f.path}`, src: streamFileUrl(f.path, root), ...guessMeta(f.path) };
}

export const canPlayTransfer = (t: Transfer) => t.direction === 'download' && t.state === 'succeeded' && isPlayable(t.filename);
