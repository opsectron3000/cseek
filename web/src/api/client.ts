import type {
  AppState,
  BrowseResult,
  Conversation,
  Directory,
  DownloadRequestItem,
  FileListing,
  FileRoot,
  Groups,
  HostListing,
  GuardState,
  Interests,
  ItemInfo,
  LidarrStatus,
  LogEntry,
  Message,
  Metrics,
  NowPlaying,
  Options,
  Recommendations,
  Room,
  RoomSummary,
  Search,
  SearchResponse,
  ShareSummary,
  SimilarUser,
  Transfer,
  TransferDirection,
  UserInfo,
  UserProfile,
  UserStats,
  UserStatusInfo,
  WishlistFilter,
  WishlistItem,
} from './types';

const TOKEN_KEY = 'cseek.token';

let token: string | null = readToken();
let unauthorizedHandler: (() => void) | null = null;

function readToken(): string | null {
  try {
    return localStorage.getItem(TOKEN_KEY);
  } catch {
    return null;
  }
}

export function getToken(): string | null {
  return token;
}

export function setToken(next: string | null): void {
  token = next;
  try {
    if (next) localStorage.setItem(TOKEN_KEY, next);
    else localStorage.removeItem(TOKEN_KEY);
  } catch {
    /* storage unavailable */
  }
}

export function onUnauthorized(handler: () => void): void {
  unauthorizedHandler = handler;
}

export class ApiError extends Error {
  status: number;
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

type Method = 'GET' | 'POST' | 'PUT' | 'DELETE';

interface RequestOptions {
  body?: unknown;
  query?: Record<string, string | number | boolean | undefined | null>;
  signal?: AbortSignal;
  /** Don't trigger the global logout on 401 (used by the session probe). */
  silent401?: boolean;
}

function buildUrl(path: string, query?: RequestOptions['query']): string {
  let url = `/api${path}`;
  if (query) {
    const params = new URLSearchParams();
    for (const [k, v] of Object.entries(query)) {
      if (v !== undefined && v !== null) params.set(k, String(v));
    }
    const s = params.toString();
    if (s) url += `?${s}`;
  }
  return url;
}

export async function request<T>(method: Method, path: string, opts: RequestOptions = {}): Promise<T> {
  const headers: Record<string, string> = { Accept: 'application/json' };
  if (token) headers.Authorization = `Bearer ${token}`;
  let body: string | undefined;
  if (opts.body !== undefined) {
    headers['Content-Type'] = 'application/json';
    body = JSON.stringify(opts.body);
  }
  let res: Response;
  try {
    res = await fetch(buildUrl(path, opts.query), { method, headers, body, signal: opts.signal });
  } catch (e) {
    if (e instanceof DOMException && e.name === 'AbortError') throw e;
    throw new ApiError(0, 'Daemon unreachable');
  }
  if (res.status === 401 && !opts.silent401) unauthorizedHandler?.();
  if (!res.ok) {
    let message = `${res.status} ${res.statusText}`;
    try {
      const data = (await res.json()) as { error?: string };
      if (data && typeof data.error === 'string') message = data.error;
    } catch {
      /* non-JSON error */
    }
    throw new ApiError(res.status, message);
  }
  if (res.status === 204) return undefined as T;
  const text = await res.text();
  if (!text) return undefined as T;
  return JSON.parse(text) as T;
}

/** Multipart upload into a folder; resolves with the saved relative paths. */
async function upload(root: string, path: string, files: File[]): Promise<string[]> {
  const form = new FormData();
  for (const f of files) form.append('file', f, f.name);
  const headers: Record<string, string> = { Accept: 'application/json' };
  if (token) headers.Authorization = `Bearer ${token}`;
  let res: Response;
  try {
    res = await fetch(buildUrl('/files/upload', { root, path }), { method: 'POST', headers, body: form });
  } catch {
    throw new ApiError(0, 'Daemon unreachable');
  }
  if (res.status === 401) unauthorizedHandler?.();
  if (!res.ok) {
    let message = `${res.status} ${res.statusText}`;
    try {
      const data = (await res.json()) as { error?: string };
      if (typeof data?.error === 'string') message = data.error;
    } catch {
      /* non-JSON error */
    }
    throw new ApiError(res.status, message);
  }
  return (await res.json()) as string[];
}

const enc = encodeURIComponent;

/** URL usable in <audio src> / <img src>, authenticated via ?token=. */
export function mediaUrl(path: string, query: Record<string, string> = {}): string {
  const params = new URLSearchParams(query);
  if (token) params.set('token', token);
  const s = params.toString();
  return `/api${path}${s ? `?${s}` : ''}`;
}

export const streamTransferUrl = (id: string) => mediaUrl(`/stream/transfer/${enc(id)}`);
export const streamFileUrl = (path: string, root = 'downloads', download = false) =>
  mediaUrl('/stream/file', { root, path, ...(download ? { download: '1' } : {}) });
export const userPictureUrl = (username: string) => mediaUrl(`/users/${enc(username)}/picture`);

export function eventsUrl(): string {
  const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
  const q = token ? `?token=${enc(token)}` : '';
  return `${proto}//${location.host}/api/events${q}`;
}

export const api = {
  // Session / app
  login: (username: string, password: string) =>
    request<{ token: string; username: string }>('POST', '/session', { body: { username, password }, silent401: true }),
  session: () => request<{ username: string; authDisabled: boolean }>('GET', '/session', { silent401: true }),
  app: () => request<AppState>('GET', '/app'),
  connect: () => request<void>('PUT', '/server'),
  disconnect: () => request<void>('DELETE', '/server'),
  setStatus: (status: 'online' | 'away') => request<void>('PUT', '/server/status', { body: { status } }),

  // Searches
  search: (query: string) => request<Search>('POST', '/searches', { body: { query } }),
  searches: () => request<Search[]>('GET', '/searches'),
  getSearch: (id: string) => request<Search>('GET', `/searches/${enc(id)}`),
  searchResponses: (id: string, signal?: AbortSignal) =>
    request<SearchResponse[]>('GET', `/searches/${enc(id)}/responses`, { signal }),
  stopSearch: (id: string) => request<Search>('PUT', `/searches/${enc(id)}/stop`),
  deleteSearch: (id: string) => request<void>('DELETE', `/searches/${enc(id)}`),
  clearSearches: () => request<void>('DELETE', '/searches'),

  // Transfers
  transfers: (dir: TransferDirection) => request<Transfer[]>('GET', `/transfers/${dir}s`),
  download: (items: DownloadRequestItem[], destination?: string) =>
    request<Transfer[]>('POST', '/transfers/downloads', { body: destination ? { items, destination } : { items } }),
  retryDownload: (id: string) => request<Transfer>('POST', `/transfers/downloads/${enc(id)}/retry`),
  refreshPosition: (id: string) => request<Transfer>('POST', `/transfers/downloads/${enc(id)}/position`),
  cancelTransfer: (dir: TransferDirection, id: string, remove = false) =>
    request<void>('DELETE', `/transfers/${dir}s/${enc(id)}`, { query: remove ? { remove: true } : undefined }),
  clearTransfers: (dir: TransferDirection, state: 'finished' | 'succeeded') =>
    request<void>('DELETE', `/transfers/${dir}s`, { query: { state } }),

  // Users
  countries: () => request<Record<string, string>>('GET', '/users/countries'),
  userInfo: (u: string) => request<UserInfo>('GET', `/users/${enc(u)}/info`),
  userStatus: (u: string) => request<UserStatusInfo>('GET', `/users/${enc(u)}/status`),
  userStats: (u: string) => request<UserStats>('GET', `/users/${enc(u)}/stats`),
  browse: (u: string, signal?: AbortSignal) => request<BrowseResult>('GET', `/users/${enc(u)}/browse`, { signal }),
  userDirectory: (u: string, directory: string) =>
    request<Directory[]>('POST', `/users/${enc(u)}/directory`, { body: { directory } }),
  userInterests: (u: string) => request<Interests>('GET', `/users/${enc(u)}/interests`),
  userAddress: (u: string) => request<{ ip: string; port: number }>('GET', `/users/${enc(u)}/address`),
  userProfile: (u: string) => request<UserProfile>('GET', `/users/${enc(u)}/profile`),
  setUserNote: (u: string, note: string) => request<UserProfile>('PUT', `/users/${enc(u)}/note`, { body: { note } }),
  setUserGroup: (u: string, group: string | null) =>
    request<UserProfile>('PUT', `/users/${enc(u)}/group`, { body: { group } }),
  reputation: () => request<Record<string, number>>('GET', '/reputation'),

  // Conversations
  conversations: () => request<Conversation[]>('GET', '/conversations'),
  conversation: (u: string) => request<Message[]>('GET', `/conversations/${enc(u)}`),
  sendMessage: (u: string, message: string) => request<Message>('POST', `/conversations/${enc(u)}`, { body: { message } }),
  markRead: (u: string) => request<void>('PUT', `/conversations/${enc(u)}/read`),
  deleteConversation: (u: string) => request<void>('DELETE', `/conversations/${enc(u)}`),

  // Rooms
  availableRooms: () => request<RoomSummary[]>('GET', '/rooms/available'),
  joinedRooms: () => request<Room[]>('GET', '/rooms/joined'),
  room: (name: string) => request<Room>('GET', `/rooms/joined/${enc(name)}`),
  joinRoom: (name: string) => request<Room>('POST', '/rooms/joined', { body: { name } }),
  leaveRoom: (name: string) => request<void>('DELETE', `/rooms/joined/${enc(name)}`),
  sendRoomMessage: (name: string, message: string) =>
    request<void>('POST', `/rooms/joined/${enc(name)}/messages`, { body: { message } }),

  // Shares & files
  shares: () => request<ShareSummary>('GET', '/shares'),
  rescan: () => request<void>('PUT', '/shares/scan'),
  /** Share a folder inside a file root (`root` + relative `path`), or a host folder from the picker (`path` only). */
  addShare: (body: { root?: string; path: string; alias?: string }) => request<FileRoot[]>('POST', '/shares/directories', { body }),
  removeShare: (shareId: string) => request<FileRoot[]>('DELETE', '/shares/directories', { query: { root: shareId } }),
  fileRoots: () => request<FileRoot[]>('GET', '/files/roots'),
  files: (root: string, path: string, signal?: AbortSignal) =>
    request<FileListing>('GET', '/files', { query: { root, path }, signal }),
  deleteFile: (root: string, path: string) => request<void>('DELETE', '/files', { query: { root, path } }),
  mkdir: (root: string, path: string) => request<void>('POST', '/files/mkdir', { body: { root, path } }),
  moveFile: (root: string, from: string, to: string) => request<void>('POST', '/files/move', { body: { root, from, to } }),
  upload: (root: string, path: string, files: File[]) => upload(root, path, files),
  hostDirs: (path = '') => request<HostListing>('GET', '/fs/dirs', { query: { path } }),

  // Wishlist
  wishlist: () => request<WishlistItem[]>('GET', '/wishlist'),
  addWishlist: (body: { query: string; autoDownload?: boolean; filter?: WishlistFilter }) =>
    request<WishlistItem>('POST', '/wishlist', { body }),
  updateWishlist: (id: string, patch: Partial<WishlistItem>) =>
    request<WishlistItem>('PUT', `/wishlist/${enc(id)}`, { body: patch }),
  deleteWishlist: (id: string) => request<void>('DELETE', `/wishlist/${enc(id)}`),
  runWishlist: (id: string) => request<WishlistItem>('POST', `/wishlist/${enc(id)}/run`),

  // Discovery
  interests: () => request<Interests>('GET', '/discovery/interests'),
  addInterest: (item: string, kind: 'like' | 'hate') =>
    request<Interests>('POST', '/discovery/interests', { body: { item, kind } }),
  removeInterest: (item: string, kind: 'like' | 'hate') =>
    request<Interests>('DELETE', '/discovery/interests', { body: { item, kind } }),
  recommendations: () => request<Recommendations>('GET', '/discovery/recommendations'),
  globalRecommendations: () => request<Recommendations>('GET', '/discovery/global'),
  similarUsers: () => request<SimilarUser[]>('GET', '/discovery/similar-users'),
  itemInfo: (item: string) => request<ItemInfo>('GET', `/discovery/items/${enc(item)}`),

  // Now playing
  nowPlaying: () => request<NowPlaying | null>('GET', '/nowplaying'),
  setNowPlaying: (body: { artist: string; title: string; album?: string }) =>
    request<NowPlaying>('PUT', '/nowplaying', { body }),
  clearNowPlaying: () => request<void>('DELETE', '/nowplaying'),

  // Integrations
  lidarrStatus: () => request<LidarrStatus>('GET', '/integrations/lidarr/status'),
  lidarrSync: () => request<{ added: number }>('POST', '/integrations/lidarr/sync'),

  // System
  options: () => request<Options>('GET', '/options'),
  saveOptions: (opts: Options) => request<Options>('PUT', '/options', { body: opts }),
  logs: () => request<LogEntry[]>('GET', '/logs'),
  metrics: () => request<Metrics>('GET', '/metrics'),
  guard: () => request<GuardState>('GET', '/guard'),
  unban: (u: string) => request<void>('DELETE', `/guard/bans/${enc(u)}`),
  groups: () => request<Groups>('GET', '/groups'),
};
