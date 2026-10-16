import { api } from '../api/client';
import type { BrowseResult } from '../api/types';
import { createStore } from './createStore';

export interface BrowseSession {
  username: string;
  loading: boolean;
  error: string | null;
  result: BrowseResult | null;
  startedAt: number;
}

/** Browse sessions survive navigation (kept in memory for this tab). */
export const browseStore = createStore<{ sessions: BrowseSession[] }>({ sessions: [] });
export const useBrowse = browseStore.use;

function patch(username: string, p: Partial<BrowseSession>) {
  browseStore.set((s) => ({ sessions: s.sessions.map((x) => (x.username === username ? { ...x, ...p } : x)) }));
}

export function openBrowse(username: string, force = false): void {
  const existing = browseStore.get().sessions.find((s) => s.username === username);
  if (existing && !force && (existing.loading || existing.result)) return;
  if (!existing) {
    browseStore.set((s) => ({
      sessions: [...s.sessions, { username, loading: true, error: null, result: null, startedAt: Date.now() }],
    }));
  } else {
    patch(username, { loading: true, error: null, startedAt: Date.now() });
  }
  api.browse(username).then(
    (result) => patch(username, { loading: false, result }),
    (e: unknown) => patch(username, { loading: false, error: e instanceof Error ? e.message : String(e) }),
  );
}

export function closeBrowse(username: string): void {
  browseStore.set((s) => ({ sessions: s.sessions.filter((x) => x.username !== username) }));
}
