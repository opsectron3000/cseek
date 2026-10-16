import { api } from '../api/client';
import { store } from '../store/app';
import { navigate, routes } from './router';

/** Start a network search and open it. Throws on failure. */
export async function startSearch(query: string): Promise<void> {
  const s = await api.search(query);
  store.set((st) => ({ searches: { ...st.searches, [s.id]: s } }));
  navigate(routes.search(s.id));
}

export async function joinRoom(name: string): Promise<void> {
  const room = await api.joinRoom(name);
  store.set((s) => ({ rooms: { ...s.rooms, [room.name]: room } }));
  navigate(routes.room(room.name));
}
