// Types mirror docs/API.md (cseekd HTTP API v0). Keep in sync with the backend.

export type ServerState = 'disconnected' | 'connecting' | 'loggedIn' | 'loginFailed';
export type UserStatus = 'offline' | 'away' | 'online';

export interface AppState {
  version: string;
  server: {
    state: ServerState;
    username: string | null;
    address: string;
    error: string | null;
    isPrivileged: boolean;
    privilegeSecondsLeft: number;
    status: UserStatus;
    listenPort: number;
  };
  shares: ShareSummary;
  distributed: {
    hasParent: boolean;
    parent: string | null;
    branchLevel: number;
    branchRoot: string | null;
    searchesReceived: number;
  };
  transfers: {
    downloadSpeed: number;
    uploadSpeed: number;
    activeDownloads: number;
    activeUploads: number;
    queuedUploads: number;
  };
  nowPlaying: NowPlaying | null;
  unreadMessages: number;
}

export interface SearchFile {
  filename: string;
  size: number;
  extension: string;
  bitRate?: number;
  bitDepth?: number;
  sampleRate?: number;
  length?: number;
  isVariableBitRate?: boolean;
}

export type SearchState = 'inProgress' | 'completed' | 'cancelled';

export interface Search {
  id: string;
  query: string;
  state: SearchState;
  startedAt: string;
  endedAt: string | null;
  responseCount: number;
  fileCount: number;
  lockedFileCount: number;
}

export interface SearchResponse {
  username: string;
  hasFreeUploadSlot: boolean;
  uploadSpeed: number;
  queueLength: number;
  score: number;
  reputation: number;
  files: SearchFile[];
  lockedFiles: SearchFile[];
}

export type TransferState =
  | 'queued'
  | 'remoteQueued'
  | 'requested'
  | 'inProgress'
  | 'succeeded'
  | 'failed'
  | 'cancelled'
  | 'rejected';

export type TransferDirection = 'download' | 'upload';

export interface TransferSource {
  username: string;
  filename: string;
  state: 'idle' | 'queued' | 'active' | 'failed';
  bytes: number;
}

export interface Transfer {
  id: string;
  direction: TransferDirection;
  username: string;
  filename: string;
  size: number;
  state: TransferState;
  stateDetail: string | null;
  bytesTransferred: number;
  averageSpeed: number;
  placeInQueue: number | null;
  requestedAt: string;
  startedAt: string | null;
  endedAt: string | null;
  localPath: string | null;
  sources: TransferSource[];
}

export interface DownloadRequestItem {
  username: string;
  filename: string;
  size: number;
  sources?: { username: string; filename: string }[];
}

export interface UserInfo {
  description: string;
  hasPicture: boolean;
  uploadSlots: number;
  queueLength: number;
  hasFreeUploadSlot: boolean;
}
export interface UserStatusInfo {
  status: UserStatus;
  isPrivileged: boolean;
}
export interface UserStats {
  averageSpeed: number;
  uploadCount: number;
  fileCount: number;
  directoryCount: number;
}
export interface Directory {
  name: string;
  files: SearchFile[];
}
export interface BrowseResult {
  directories: Directory[];
  lockedDirectories: Directory[];
}
export interface Interests {
  likes: string[];
  hates: string[];
}

export interface Reputation {
  score: number;
  downloadsSucceeded: number;
  downloadsFailed: number;
  uploadsSucceeded: number;
  bytesDownloaded: number;
  bytesUploaded: number;
  violations: number;
}

export interface UserProfile {
  username: string;
  note: string;
  group: string | null;
  reputation: Reputation;
}

export interface Message {
  id: string;
  timestamp: string;
  username: string;
  direction: 'in' | 'out';
  message: string;
}

export interface Conversation {
  username: string;
  unread: number;
  lastMessage: Message | null;
  status: UserStatus;
}

export interface RoomUser {
  username: string;
  status: UserStatus;
  averageSpeed: number;
  fileCount: number;
  country: string | null;
}
export interface RoomMessage {
  timestamp: string;
  username: string;
  message: string;
  self: boolean;
}
export interface RoomSummary {
  name: string;
  userCount: number;
  isPrivate: boolean;
}
export interface Room {
  name: string;
  isPrivate: boolean;
  users: RoomUser[];
  messages: RoomMessage[];
  tickers: { username: string; message: string }[];
}

export interface ShareDirectorySummary {
  alias: string;
  fileCount: number;
  directoryCount: number;
}

export interface ShareSummary {
  directories: ShareDirectorySummary[];
  fileCount: number;
  directoryCount: number;
  scanning: boolean;
  scanProgress: number;
  lastScan: string | null;
}

export interface WishlistFilter {
  extensions: string[];
  minBitRate: number | null;
  /** bytes */
  minSize: number | null;
  /** bytes */
  maxSize: number | null;
}

export interface WishlistItem {
  id: string;
  query: string;
  enabled: boolean;
  autoDownload: boolean;
  filter: WishlistFilter;
  createdAt: string;
  lastRunAt: string | null;
  lastMatchCount: number;
  lastSearchId: string | null;
  downloadedCount: number;
}

export interface NowPlaying {
  artist: string;
  title: string;
  album: string | null;
  source: string;
  startedAt: string;
}

export type LogLevel = 'TRACE' | 'DEBUG' | 'INFO' | 'WARN' | 'ERROR';
export interface LogEntry {
  timestamp: string;
  level: LogLevel;
  target: string;
  message: string;
}

export interface GuardState {
  violations: { username: string; kind: string; count: number; lastAt: string }[];
  bans: { username: string; reason: string; until: string | null }[];
}

export interface Metrics {
  uptimeSeconds: number;
  bytesDownloaded: number;
  bytesUploaded: number;
  downloadsSucceeded: number;
  downloadsFailed: number;
  uploadsSucceeded: number;
  uploadsFailed: number;
  searchesSent: number;
  searchRequestsReceived: number;
  searchResponsesSent: number;
  peerConnections: number;
}

export interface FileEntry {
  name: string;
  path: string;
  isDirectory: boolean;
  size: number;
  modifiedAt: string;
  /** Inside a folder shared on the network. */
  shared: boolean;
}

export type FileRootKind = 'storage' | 'downloads' | 'incomplete' | 'destination' | 'share';

/** A top-level folder the file manager can browse. */
export interface FileRoot {
  /** `storage` (everything), `downloads`, `incomplete`, `dest-<name>` or `share-<index>`. */
  id: string;
  label: string;
  kind: FileRootKind;
  shared: boolean;
  exists: boolean;
  fileCount?: number;
  directoryCount?: number;
}

export interface FileListing {
  root: string;
  /** Relative to the root, '/'-separated; '' is the root itself. */
  path: string;
  shared: boolean;
  /** Set when this folder is itself a shared root. */
  shareId?: string;
  entries: FileEntry[];
}

export interface HostDir {
  name: string;
  path: string;
}

/** A folder inside storage, for the share folder picker. Paths are relative; '' is storage itself. */
export interface HostListing {
  path: string;
  parent: string | null;
  dirs: HostDir[];
}

export interface GroupConfig {
  name: string;
  members: string[];
  uploadSlots: number;
  speedLimitKbps: number;
  priority: number;
  allowedExtensions: string[];
  queueLimitFiles: number;
}

export interface Groups {
  builtin: string[];
  custom: GroupConfig[];
}

export interface ScoredItem {
  item: string;
  score: number;
}
export interface Recommendations {
  recommendations: ScoredItem[];
  unrecommendations: ScoredItem[];
}
export interface SimilarUser {
  username: string;
  rating: number;
}
export interface ItemInfo {
  recommendations: ScoredItem[];
  similarUsers: string[];
}

export interface LidarrStatus {
  enabled: boolean;
  lastSync: string | null;
  lastError: string | null;
  lastAdded: number;
}

export interface Options {
  soulseek: {
    username: string;
    password: string;
    serverHost: string;
    serverPort: number;
    listenPort: number;
    description: string;
    picture: string | null;
    autoConnect: boolean;
  };
  directories: { downloads: string; incomplete: string; destinations: { name: string; path: string }[] };
  shares: { directories: string[]; excludeFilters: string[]; rescanIntervalMinutes: number };
  transfers: {
    uploadSlots: number;
    uploadSpeedLimitKbps: number;
    downloadSpeedLimitKbps: number;
    maxQueuedFilesPerUser: number;
    maxQueuedBytesPerUser: number;
    multiSource: boolean;
    multiSourceChunkMb: number;
    stalledTimeoutSeconds: number;
    autoReplaceStalled: boolean;
  };
  groups: GroupConfig[];
  blacklist: string[];
  leechers: { enabled: boolean; minFiles: number; minDirectories: number; speedLimitKbps: number; allowedExtensions: string[] };
  search: { responseLimit: number; fileLimit: number; timeoutSeconds: number; maxResultsPerResponse: number; excludedPhrases: string[] };
  rooms: { autoJoin: string[] };
  wishlist: { enabled: boolean; intervalMinutes: number; maxAutoDownloadsPerRun: number };
  guard: { enabled: boolean; maxQueueRequestsPerMinute: number; maxSearchResponsesPerMinute: number; banThreshold: number; banMinutes: number };
  nowPlaying: { enabled: boolean; descriptionTemplate: string };
  integrations: { lidarr: { enabled: boolean; url: string; apiKey: string; syncIntervalMinutes: number; importOnComplete: boolean } };
  web: { port: number; host: string; auth: { disabled: boolean; username: string; password: string; apiKeys: string[] } };
}

export type ServerEvent =
  | { type: 'app'; data: AppState }
  | { type: 'search.updated'; data: Search }
  | { type: 'search.removed'; data: { id: string } }
  | { type: 'transfer.updated'; data: Transfer }
  | { type: 'transfer.removed'; data: { id: string } }
  | { type: 'message'; data: Message }
  | { type: 'room.message'; data: { room: string; message: RoomMessage } }
  | { type: 'room.updated'; data: Room }
  | { type: 'room.left'; data: { name: string } }
  | { type: 'wishlist.updated'; data: WishlistItem }
  | { type: 'log'; data: LogEntry }
  | { type: 'user.country'; data: { username: string; country: string } };
