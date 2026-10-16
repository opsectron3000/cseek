import { useSyncExternalStore } from 'react';

export interface Store<S> {
  get: () => S;
  set: (patch: Partial<S> | ((s: S) => Partial<S>)) => void;
  subscribe: (listener: () => void) => () => void;
  use: <T>(selector: (s: S) => T) => T;
}

/** Minimal external store. Selectors must return stable references (slices, primitives). */
export function createStore<S extends object>(initial: S): Store<S> {
  let state = initial;
  const listeners = new Set<() => void>();
  const get = () => state;
  const set: Store<S>['set'] = (patch) => {
    const next = typeof patch === 'function' ? patch(state) : patch;
    state = { ...state, ...next };
    listeners.forEach((l) => l());
  };
  const subscribe = (l: () => void) => {
    listeners.add(l);
    return () => {
      listeners.delete(l);
    };
  };
  const use = <T,>(selector: (s: S) => T): T => useSyncExternalStore(subscribe, () => selector(state));
  return { get, set, subscribe, use };
}
