import { useSyncExternalStore } from 'react';

const listeners = new Set<() => void>();
const emit = () => listeners.forEach((l) => l());

window.addEventListener('popstate', emit);

function subscribe(l: () => void) {
  listeners.add(l);
  return () => listeners.delete(l);
}

const getPath = () => location.pathname;
const getSearch = () => location.search;

export function navigate(to: string, opts: { replace?: boolean } = {}): void {
  const current = location.pathname + location.search;
  if (to === current) return;
  if (opts.replace) history.replaceState(null, '', to);
  else history.pushState(null, '', to);
  emit();
}

export function usePath(): string {
  return useSyncExternalStore(subscribe, getPath);
}

export function useQuery(): URLSearchParams {
  const s = useSyncExternalStore(subscribe, getSearch);
  return new URLSearchParams(s);
}

/** Match "/users/:name" style patterns. Returns decoded params or null. */
export function matchPath(pattern: string, path: string): Record<string, string> | null {
  const p = pattern.split('/').filter(Boolean);
  const s = path.split('/').filter(Boolean);
  const params: Record<string, string> = {};
  for (let i = 0; i < p.length; i++) {
    const seg = p[i];
    if (seg.endsWith('*')) {
      params[seg.slice(1, -1) || 'rest'] = s.slice(i).map(decodeURIComponent).join('/');
      return params;
    }
    const optional = seg.endsWith('?');
    const name = optional ? seg.slice(1, -1) : seg.slice(1);
    if (i >= s.length) {
      if (optional) continue;
      return null;
    }
    if (seg.startsWith(':')) params[name] = decodeURIComponent(s[i]);
    else if (seg !== s[i]) return null;
  }
  if (s.length > p.length) return null;
  return params;
}

export const routes = {
  search: (id?: string) => (id ? `/search/${encodeURIComponent(id)}` : '/search'),
  user: (name: string) => `/users/${encodeURIComponent(name)}`,
  browse: (name: string) => `/users/${encodeURIComponent(name)}?tab=browse`,
  dm: (name: string) => `/chat/u/${encodeURIComponent(name)}`,
  room: (name: string) => `/chat/r/${encodeURIComponent(name)}`,
};

export const FOCUS_SEARCH_EVENT = 'cseek:focus-search';
