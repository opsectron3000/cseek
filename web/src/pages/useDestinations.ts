import { useEffect, useState } from 'react';
import { api } from '../api/client';

type Destination = { name: string; path: string };

let cache: Destination[] | null = null;
let inflight: Promise<Destination[]> | null = null;

/** Named download destinations from options.directories.destinations (cached for the session). */
export function useDestinations(): Destination[] {
  const [list, setList] = useState<Destination[]>(cache ?? []);
  useEffect(() => {
    if (cache) return;
    inflight ??= api
      .options()
      .then((o) => (cache = o.directories?.destinations ?? []))
      .catch(() => (cache = []));
    let alive = true;
    void inflight.then((d) => alive && setList(d));
    return () => {
      alive = false;
    };
  }, []);
  return list;
}

export function invalidateDestinations(): void {
  cache = null;
  inflight = null;
}
