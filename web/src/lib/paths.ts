/** Soulseek remote paths are backslash separated. Accept "/" too for safety. */
export function splitRemote(path: string): string[] {
  return path.split(/[\\/]/).filter(Boolean);
}

export function basename(path: string): string {
  const i = Math.max(path.lastIndexOf('\\'), path.lastIndexOf('/'));
  return i >= 0 ? path.slice(i + 1) : path;
}

export function dirname(path: string): string {
  const i = Math.max(path.lastIndexOf('\\'), path.lastIndexOf('/'));
  return i >= 0 ? path.slice(0, i) : '';
}

export function extension(path: string): string {
  const b = basename(path);
  const i = b.lastIndexOf('.');
  return i > 0 ? b.slice(i + 1).toLowerCase() : '';
}

/** Last N segments of a directory path, for compact display. */
export function shortDir(path: string, segments = 2): string {
  const parts = splitRemote(path);
  if (parts.length <= segments) return parts.join('\\');
  return `…\\${parts.slice(-segments).join('\\')}`;
}

export function stripExtension(name: string): string {
  const i = name.lastIndexOf('.');
  return i > 0 ? name.slice(0, i) : name;
}

const PLAYABLE = new Set(['mp3', 'flac', 'ogg', 'oga', 'opus', 'm4a', 'aac', 'wav', 'webm', 'mp4']);
const AUDIO = new Set([...PLAYABLE, 'ape', 'wv', 'alac', 'aiff', 'aif', 'wma', 'dsf', 'dff', 'mpc']);
export const LOSSLESS = new Set(['flac', 'wav', 'ape', 'wv', 'alac', 'aiff', 'aif', 'dsf', 'dff']);

export const isPlayable = (path: string) => PLAYABLE.has(extension(path));
export const isAudioExt = (ext: string) => AUDIO.has(ext.toLowerCase());

/** Join a local relative path (downloads root, forward slashes). */
export function joinLocal(dir: string, name: string): string {
  return dir ? `${dir.replace(/\/+$/, '')}/${name}` : name;
}

export function parentLocal(path: string): string {
  const i = path.lastIndexOf('/');
  return i >= 0 ? path.slice(0, i) : '';
}
