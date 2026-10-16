import { useSyncExternalStore } from 'react';

export type ThemePref = 'system' | 'dark' | 'light';
const KEY = 'cseek:theme';
const media = matchMedia('(prefers-color-scheme: light)');
const listeners = new Set<() => void>();

function read(): ThemePref {
  try {
    const v = localStorage.getItem(KEY);
    return v === 'dark' || v === 'light' ? v : 'system';
  } catch {
    return 'system';
  }
}

let pref: ThemePref = read();

function apply(): void {
  const dark = pref === 'dark' || (pref === 'system' && !media.matches);
  document.documentElement.dataset.theme = dark ? 'dark' : 'light';
  document.querySelector('meta[name="theme-color"]')?.setAttribute('content', dark ? '#0a0909' : '#f0edeb');
}

media.addEventListener('change', () => {
  if (pref === 'system') {
    apply();
    listeners.forEach((l) => l());
  }
});
apply();

export function setTheme(next: ThemePref): void {
  pref = next;
  try {
    if (next === 'system') localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, next);
  } catch {
    /* ignore */
  }
  apply();
  listeners.forEach((l) => l());
}

export function resolvedTheme(): 'dark' | 'light' {
  return document.documentElement.dataset.theme === 'light' ? 'light' : 'dark';
}

/** Flip between dark and light (leaving "system"). */
export function toggleTheme(): void {
  setTheme(resolvedTheme() === 'dark' ? 'light' : 'dark');
}

function subscribe(l: () => void) {
  listeners.add(l);
  return () => listeners.delete(l);
}

export function useTheme(): ThemePref {
  return useSyncExternalStore(subscribe, () => pref);
}
