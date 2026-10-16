import { api, getToken, onUnauthorized, setToken } from '../api/client';
import { connectEvents } from '../api/events';
import type {
  AppState,
  Conversation,
  LogEntry,
  Message,
  Room,
  Search,
  ServerEvent,
  Transfer,
  WishlistItem,
} from '../api/types';
import { createStore } from './createStore';

export type AuthStatus = 'checking' | 'login' | 'ready';

export interface RootState {
  auth: { status: AuthStatus; username: string | null; authDisabled: boolean };
  connected: boolean;
  app: AppState | null;
  searches: Record<string, Search>;
  transfers: Record<string, Transfer>;
  transfersLoaded: boolean;
  conversations: Record<string, Conversation>;
  /** Loaded threads, oldest first. */
  messages: Record<string, Message[]>;
  rooms: Record<string, Room>;
  roomUnread: Record<string, number>;
  /** "u:<name>" or "r:<name>" — the chat currently visible, to suppress unread counts. */
  activeChat: string | null;
  wishlist: Record<string, WishlistItem>;
  logs: LogEntry[];
  /** Users we know are blacklisted (from options.blacklist + fetched profiles). */
  blacklist: Set<string>;
  /** username → ISO country code (server-reported or GeoIP). */
  countries: Record<string, string>;
}

const LOG_CAP = 2000;

export const store = createStore<RootState>({
  auth: { status: 'checking', username: null, authDisabled: false },
  connected: false,
  app: null,
  searches: {},
  transfers: {},
  transfersLoaded: false,
  conversations: {},
  messages: {},
  rooms: {},
  roomUnread: {},
  activeChat: null,
  wishlist: {},
  logs: [],
  blacklist: new Set(),
  countries: {},
});

export const useStore = store.use;

const byKey = <T,>(items: T[], key: (t: T) => string): Record<string, T> => {
  const out: Record<string, T> = {};
  for (const it of items) out[key(it)] = it;
  return out;
};

// ---------- event handling ----------

function handleEvent(ev: ServerEvent): void {
  switch (ev.type) {
    case 'app':
      store.set({ app: ev.data });
      return;
    case 'search.updated':
      store.set((s) => ({ searches: { ...s.searches, [ev.data.id]: ev.data } }));
      return;
    case 'search.removed':
      store.set((s) => {
        const { [ev.data.id]: _gone, ...rest } = s.searches;
        return { searches: rest };
      });
      return;
    case 'transfer.updated':
      store.set((s) => ({ transfers: { ...s.transfers, [ev.data.id]: ev.data } }));
      return;
    case 'transfer.removed':
      store.set((s) => {
        const { [ev.data.id]: _gone, ...rest } = s.transfers;
        return { transfers: rest };
      });
      return;
    case 'message':
      receiveMessage(ev.data);
      return;
    case 'room.message': {
      const { room, message } = ev.data;
      store.set((s) => {
        const existing = s.rooms[room];
        if (!existing) return {};
        const active = s.activeChat === `r:${room}`;
        return {
          rooms: { ...s.rooms, [room]: { ...existing, messages: [...existing.messages, message].slice(-1000) } },
          roomUnread: active || message.self ? s.roomUnread : { ...s.roomUnread, [room]: (s.roomUnread[room] ?? 0) + 1 },
        };
      });
      return;
    }
    case 'room.updated':
      store.set((s) => {
        const prev = s.rooms[ev.data.name];
        const messages = ev.data.messages.length ? ev.data.messages : (prev?.messages ?? []);
        return { rooms: { ...s.rooms, [ev.data.name]: { ...ev.data, messages } } };
      });
      return;
    case 'room.left':
      store.set((s) => {
        const { [ev.data.name]: _gone, ...rest } = s.rooms;
        const { [ev.data.name]: _u, ...unread } = s.roomUnread;
        return { rooms: rest, roomUnread: unread };
      });
      return;
    case 'wishlist.updated':
      store.set((s) => ({ wishlist: { ...s.wishlist, [ev.data.id]: ev.data } }));
      return;
    case 'user.country':
      store.set((s) =>
        s.countries[ev.data.username] === ev.data.country ? {} : { countries: { ...s.countries, [ev.data.username]: ev.data.country } },
      );
      return;
    case 'log':
      store.set((s) => {
        const logs = s.logs.length >= LOG_CAP ? s.logs.slice(-LOG_CAP + 1) : s.logs.slice();
        logs.push(ev.data);
        return { logs };
      });
      return;
  }
}

function receiveMessage(m: Message): void {
  store.set((s) => {
    const thread = s.messages[m.username];
    const messages =
      thread && !thread.some((x) => x.id === m.id) ? { ...s.messages, [m.username]: [...thread, m] } : s.messages;
    const active = s.activeChat === `u:${m.username}`;
    const prev = s.conversations[m.username];
    const conv: Conversation = {
      username: m.username,
      status: prev?.status ?? 'offline',
      lastMessage: m,
      unread: m.direction === 'in' && !active ? (prev?.unread ?? 0) + 1 : (prev?.unread ?? 0),
    };
    return { messages, conversations: { ...s.conversations, [m.username]: conv } };
  });
  if (m.direction === 'in' && store.get().activeChat === `u:${m.username}`) {
    void api.markRead(m.username).catch(() => {});
  }
}

// ---------- loaders ----------

export async function loadSearches(): Promise<void> {
  const list = await api.searches();
  store.set({ searches: byKey(list, (s) => s.id) });
}

export async function loadTransfers(): Promise<void> {
  const [down, up] = await Promise.all([api.transfers('download'), api.transfers('upload')]);
  store.set({ transfers: byKey([...down, ...up], (t) => t.id), transfersLoaded: true });
}

export async function loadConversations(): Promise<void> {
  const list = await api.conversations();
  store.set((s) => {
    // The open thread is being read right now; don't resurrect its unread count.
    const active = s.activeChat?.startsWith('u:') ? s.activeChat.slice(2) : null;
    return { conversations: byKey(list.map((c) => (c.username === active ? { ...c, unread: 0 } : c)), (c) => c.username) };
  });
}

export async function loadThread(username: string): Promise<void> {
  const list = await api.conversation(username);
  store.set((s) => ({ messages: { ...s.messages, [username]: list } }));
}

export async function loadRooms(): Promise<void> {
  const list = await api.joinedRooms();
  store.set({ rooms: byKey(list, (r) => r.name) });
}

export async function loadWishlist(): Promise<void> {
  const list = await api.wishlist();
  store.set({ wishlist: byKey(list, (w) => w.id) });
}

export async function loadLogs(): Promise<void> {
  const list = await api.logs();
  store.set((s) => {
    // Merge: keep any live entries newer than the snapshot's last timestamp.
    const last = list.length ? list[list.length - 1].timestamp : '';
    const live = s.logs.filter((l) => l.timestamp > last);
    return { logs: [...list, ...live].slice(-LOG_CAP) };
  });
}

export async function loadBlacklist(): Promise<void> {
  try {
    const opts = await api.options();
    store.set({ blacklist: new Set(opts.blacklist ?? []) });
  } catch {
    /* options may be restricted */
  }
}

export function noteUserGroup(username: string, group: string | null): void {
  store.set((s) => {
    const has = s.blacklist.has(username);
    if ((group === 'blacklisted') === has) return {};
    const next = new Set(s.blacklist);
    if (group === 'blacklisted') next.add(username);
    else next.delete(username);
    return { blacklist: next };
  });
}

export function upsertTransfers(list: Transfer[]): void {
  store.set((s) => {
    const next = { ...s.transfers };
    for (const t of list) next[t.id] = t;
    return { transfers: next };
  });
}

export function removeTransferLocal(id: string): void {
  store.set((s) => {
    const { [id]: _gone, ...rest } = s.transfers;
    return { transfers: rest };
  });
}

export function setActiveChat(key: string | null): void {
  store.set((s) => {
    if (!key) return { activeChat: null };
    if (key.startsWith('u:')) {
      const name = key.slice(2);
      const c = s.conversations[name];
      return {
        activeChat: key,
        conversations: c && c.unread ? { ...s.conversations, [name]: { ...c, unread: 0 } } : s.conversations,
      };
    }
    const room = key.slice(2);
    return { activeChat: key, roomUnread: s.roomUnread[room] ? { ...s.roomUnread, [room]: 0 } : s.roomUnread };
  });
}

async function resync(): Promise<void> {
  await Promise.allSettled([
    api.app().then((app) => store.set({ app })),
    loadSearches(),
    loadTransfers(),
    loadConversations(),
    loadRooms(),
    loadWishlist(),
    api.countries().then((countries) => store.set((s) => ({ countries: { ...s.countries, ...countries } }))),
  ]);
}

// ---------- session lifecycle ----------

let closeEvents: (() => void) | null = null;

function startSession(username: string, authDisabled: boolean): void {
  store.set({ auth: { status: 'ready', username, authDisabled } });
  closeEvents?.();
  closeEvents = connectEvents({
    onEvent: handleEvent,
    onStatus: (connected) => store.set({ connected }),
    onOpen: () => void resync(),
  });
  void resync();
  void loadBlacklist();
}

export function logout(): void {
  closeEvents?.();
  closeEvents = null;
  setToken(null);
  store.set({
    auth: { status: 'login', username: null, authDisabled: false },
    connected: false,
    app: null,
    searches: {},
    transfers: {},
    transfersLoaded: false,
    conversations: {},
    messages: {},
    rooms: {},
    roomUnread: {},
    wishlist: {},
    logs: [],
  });
}

onUnauthorized(() => {
  if (store.get().auth.status === 'ready' && !store.get().auth.authDisabled) logout();
});

export async function bootstrap(): Promise<void> {
  try {
    const s = await api.session();
    startSession(s.username, s.authDisabled);
  } catch {
    if (getToken()) setToken(null);
    store.set({ auth: { status: 'login', username: null, authDisabled: false } });
  }
}

export async function login(username: string, password: string): Promise<void> {
  const res = await api.login(username, password);
  setToken(res.token);
  startSession(res.username, false);
}
