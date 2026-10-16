import { createStore } from './createStore';
import { load, save } from '../lib/storage';

export interface Track {
  key: string;
  src: string;
  title: string;
  artist?: string;
  album?: string;
}

interface PlayerState {
  queue: Track[];
  index: number; // -1 when nothing loaded
  playing: boolean;
  currentTime: number;
  duration: number;
  volume: number;
  error: string | null;
}

export const player = createStore<PlayerState>({
  queue: [],
  index: -1,
  playing: false,
  currentTime: 0,
  duration: 0,
  volume: load('volume', 0.8),
  error: null,
});

export const usePlayer = player.use;

const audio = new Audio();
audio.preload = 'metadata';
audio.volume = player.get().volume;

audio.addEventListener('play', () => player.set({ playing: true, error: null }));
audio.addEventListener('pause', () => player.set({ playing: false }));
audio.addEventListener('timeupdate', () => player.set({ currentTime: audio.currentTime }));
audio.addEventListener('durationchange', () =>
  player.set({ duration: Number.isFinite(audio.duration) ? audio.duration : 0 }),
);
audio.addEventListener('ended', () => next());
audio.addEventListener('error', () => player.set({ playing: false, error: 'Unable to play this file' }));

function current(): Track | null {
  const s = player.get();
  return s.queue[s.index] ?? null;
}

function load_(index: number, autoplay = true): void {
  const t = player.get().queue[index];
  if (!t) return;
  player.set({ index, currentTime: 0, duration: 0, error: null });
  audio.src = t.src;
  updateMediaSession(t);
  if (autoplay) void audio.play().catch(() => player.set({ playing: false }));
}

function updateMediaSession(t: Track): void {
  if (!('mediaSession' in navigator)) return;
  navigator.mediaSession.metadata = new MediaMetadata({ title: t.title, artist: t.artist ?? '', album: t.album ?? '' });
}

if ('mediaSession' in navigator) {
  const ms = navigator.mediaSession;
  const set = (action: MediaSessionAction, handler: MediaSessionActionHandler) => {
    try {
      ms.setActionHandler(action, handler);
    } catch {
      /* unsupported action */
    }
  };
  set('play', () => void audio.play());
  set('pause', () => audio.pause());
  set('nexttrack', () => next());
  set('previoustrack', () => prev());
  set('seekto', (d) => {
    if (d.seekTime !== undefined) seek(d.seekTime);
  });
}

/** Replace queue with a single track and play it now. */
export function playNow(track: Track): void {
  const s = player.get();
  const at = s.index + 1;
  const queue = [...s.queue.slice(0, at), track, ...s.queue.slice(at)];
  player.set({ queue });
  load_(at);
}

export function playNext(track: Track): void {
  const s = player.get();
  if (s.index < 0) return playNow(track);
  const at = s.index + 1;
  player.set({ queue: [...s.queue.slice(0, at), track, ...s.queue.slice(at)] });
}

export function enqueue(tracks: Track | Track[]): void {
  const list = Array.isArray(tracks) ? tracks : [tracks];
  const s = player.get();
  player.set({ queue: [...s.queue, ...list] });
  if (s.index < 0) load_(s.queue.length);
}

export function playAt(index: number): void {
  load_(index);
}

export function removeAt(index: number): void {
  const s = player.get();
  const queue = s.queue.filter((_, i) => i !== index);
  if (index === s.index) {
    audio.pause();
    player.set({ queue });
    if (queue[index]) load_(index);
    else stop();
  } else {
    player.set({ queue, index: index < s.index ? s.index - 1 : s.index });
  }
}

export function toggle(): void {
  if (!current()) return;
  if (audio.paused) void audio.play().catch(() => {});
  else audio.pause();
}

export function next(): void {
  const s = player.get();
  if (s.index + 1 < s.queue.length) load_(s.index + 1);
  else {
    audio.pause();
    player.set({ playing: false });
  }
}

export function prev(): void {
  const s = player.get();
  if (audio.currentTime > 3 || s.index <= 0) seek(0);
  else load_(s.index - 1);
}

export function seek(t: number): void {
  audio.currentTime = t;
  player.set({ currentTime: t });
}

export function setVolume(v: number): void {
  audio.volume = v;
  player.set({ volume: v });
  save('volume', v);
}

export function stop(): void {
  audio.pause();
  audio.removeAttribute('src');
  audio.load();
  player.set({ queue: [], index: -1, playing: false, currentTime: 0, duration: 0, error: null });
  if ('mediaSession' in navigator) navigator.mediaSession.metadata = null;
}
