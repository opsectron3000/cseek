import { memo, useCallback, useEffect, useMemo, useRef, useState, type FormEvent, type KeyboardEvent, type MouseEvent } from 'react';
import { Popover } from '@base-ui/react/popover';
import { api } from '../api/client';
import type { Search, SearchFile, SearchResponse } from '../api/types';
import { loadSearches, store, upsertTransfers, useStore } from '../store/app';
import { startSearch } from '../lib/actions';
import { FILTER_HELP, findAlternates, groupResponses, parseFilter } from '../lib/filters';
import { sortFolders, toFolders, type FolderResult, type SortKey } from '../lib/results';
import { formatBytes, formatDuration, formatQuality, formatRelative, formatSpeed, plural } from '../lib/format';
import { basename } from '../lib/paths';
import { FOCUS_SEARCH_EVENT, navigate, routes } from '../lib/router';
import { load, save } from '../lib/storage';
import { Link } from '../components/Link';
import { Icon } from '../components/Icon';
import { ConfirmDialog } from '../components/Dialogs';
import { attempt, toast, toastError } from '../components/Toaster';
import { Button, ErrorNote, IconButton, Kbd, Select, Tip, User } from '../components/ui';
import { useDestinations } from './useDestinations';
import './search.css';

const PAGE = 120;
const TAB_LIMIT = 8;

type FileKey = string;
const fileKey = (u: string, f: string): FileKey => `${u}\u0000${f}`;

/* ---------------------------------------------------------------- data */

/** Responses for a search, refetched (throttled) while the search updates. */
function useResponses(search: Search | undefined) {
  const [state, setState] = useState<{ id: string | null; data: SearchResponse[]; loading: boolean; error: unknown }>({
    id: null,
    data: [],
    loading: false,
    error: null,
  });
  const lastFetch = useRef(0);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const id = search?.id;
  const version = search ? `${search.responseCount}:${search.state}` : '';

  const fetchNow = useCallback(async (sid: string) => {
    lastFetch.current = Date.now();
    try {
      const data = await api.searchResponses(sid);
      setState((s) => (s.id === sid ? { ...s, data, loading: false, error: null } : s));
    } catch (e) {
      setState((s) => (s.id === sid ? { ...s, loading: false, error: e } : s));
    }
  }, []);

  useEffect(() => {
    if (!id) return;
    setState({ id, data: [], loading: true, error: null });
    void fetchNow(id);
  }, [id, fetchNow]);

  useEffect(() => {
    if (!id || !version) return;
    const wait = Math.max(0, 1500 - (Date.now() - lastFetch.current));
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => void fetchNow(id), wait);
    return () => {
      if (timer.current) clearTimeout(timer.current);
    };
  }, [id, version, fetchNow]);

  return state.id === id ? state : { id: null, data: [], loading: !!id, error: null };
}

function useSearchList() {
  const searches = useStore((s) => s.searches);
  return useMemo(() => Object.values(searches).sort((a, b) => b.startedAt.localeCompare(a.startedAt)), [searches]);
}

async function removeSearch(id: string, activeId?: string) {
  await attempt(async () => {
    await api.deleteSearch(id);
    store.set((st) => {
      const { [id]: _gone, ...rest } = st.searches;
      return { searches: rest };
    });
    if (id === activeId) navigate(routes.search());
  });
}

/* ---------------------------------------------------------------- top: query + tabs */

function SearchBar({ autoFocus, home }: { autoFocus: boolean; home?: boolean }) {
  const [query, setQuery] = useState('');
  const [busy, setBusy] = useState(false);
  const ref = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const focus = () => ref.current?.focus();
    window.addEventListener(FOCUS_SEARCH_EVENT, focus);
    return () => window.removeEventListener(FOCUS_SEARCH_EVENT, focus);
  }, []);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    const q = query.trim();
    if (!q || busy) return;
    setBusy(true);
    try {
      await startSearch(q);
      setQuery('');
    } catch (err) {
      toastError(err, 'Search failed');
    } finally {
      setBusy(false);
    }
  };

  return (
    <form className={`sq${home ? ' sq-home' : ''}`} onSubmit={submit} role="search">
      <Icon name="search" size={home ? 20 : 18} className="icon sq-icon" />
      <input
        ref={ref}
        className="sq-input"
        placeholder="Search for an artist, album or track"
        aria-label="Search query"
        aria-keyshortcuts="/"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        onKeyDown={(e) => e.key === 'Escape' && ref.current?.blur()}
        autoFocus={autoFocus}
        autoComplete="off"
        spellCheck={false}
      />
      <span className="sq-hint">{query.trim() ? busy ? 'Starting…' : <><Kbd>↵</Kbd> to search</> : <span className="sq-idle"><Kbd>/</Kbd></span>}</span>
    </form>
  );
}

function ClearAll({ className }: { className: string }) {
  const [confirm, setConfirm] = useState(false);
  return (
    <>
      <button type="button" className={className} onClick={() => setConfirm(true)}>
        Clear all
      </button>
      <ConfirmDialog
        open={confirm}
        onOpenChange={setConfirm}
        title="Clear all searches?"
        description="All searches and their results will be removed."
        confirmLabel="Clear all"
        danger
        onConfirm={async () => {
          await attempt(async () => {
            await api.clearSearches();
            store.set({ searches: {} });
            navigate(routes.search());
          });
        }}
      />
    </>
  );
}

function SearchTabs({ list, activeId }: { list: Search[]; activeId?: string }) {
  if (!list.length) return null;
  // One tab per query (newest wins), and keep the active search visible even
  // when it's older than the tab limit.
  const seen = new Set<string>();
  const unique = list.filter((s) => {
    const q = s.query.trim().toLowerCase();
    if (seen.has(q) && s.id !== activeId) return false;
    seen.add(q);
    return true;
  });
  const shown = unique.slice(0, TAB_LIMIT);
  const active = list.find((s) => s.id === activeId);
  if (active && !shown.includes(active)) shown[shown.length - 1] = active;

  return (
    <nav className="stabs" aria-label="Searches">
      {shown.map((s) => (
        <span key={s.id} className={`stab${s.id === activeId ? ' on' : ''}`}>
          <Link to={routes.search(s.id)} className="stab-link" aria-current={s.id === activeId ? 'page' : undefined}>
            {s.state === 'inProgress' && <span className="stab-live" aria-label="searching" />}
            <span className="stab-q">{s.query}</span>
            <span className="stab-n">{s.responseCount.toLocaleString()}</span>
          </Link>
          <button type="button" className="stab-x" aria-label={`Close ${s.query}`} onClick={() => void removeSearch(s.id, activeId)}>
            <Icon name="x" size={12} />
          </button>
        </span>
      ))}
      {unique.length > TAB_LIMIT && <span className="stab-more">+{unique.length - TAB_LIMIT} older</span>}
      <ClearAll className="stab-clear" />
    </nav>
  );
}

/* ---------------------------------------------------------------- filters */

const TOGGLES: { token: string; label: string; hint: string }[] = [
  { token: 'islossless', label: 'Lossless', hint: 'FLAC, WAV, ALAC and other lossless files' },
  { token: 'minbr:320', label: '320+', hint: 'Bitrate of at least 320 kbps' },
  { token: 'minfif:5', label: 'Albums', hint: 'Folders with at least 5 matching files' },
  { token: 'isfree', label: 'Free slot', hint: 'Only users who can upload right away' },
];

function toggleToken(text: string, token: string): string {
  const parts = text.split(/\s+/).filter(Boolean);
  return (parts.includes(token) ? parts.filter((p) => p !== token) : [...parts, token]).join(' ');
}

function FilterHelp() {
  return (
    <Popover.Root>
      <Popover.Trigger className="sf-help" aria-label="Filter syntax">
        <Icon name="info" size={15} />
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Positioner className="popover-positioner" sideOffset={8} align="start">
          <Popover.Popup className="popover sf-help-pop">
            <Popover.Title className="popover-title">Filter syntax</Popover.Title>
            <dl className="sf-help-list">
              {FILTER_HELP.map(([k, v]) => (
                <div key={k}>
                  <dt className="mono">{k}</dt>
                  <dd>{v}</dd>
                </div>
              ))}
            </dl>
          </Popover.Popup>
        </Popover.Positioner>
      </Popover.Portal>
    </Popover.Root>
  );
}

function FilterBar({
  text,
  setText,
  showLocked,
  setShowLocked,
  hideBlacklisted,
  setHideBlacklisted,
}: {
  text: string;
  setText: (fn: (t: string) => string) => void;
  showLocked: boolean;
  setShowLocked: (v: boolean) => void;
  hideBlacklisted: boolean;
  setHideBlacklisted: (v: boolean) => void;
}) {
  const tokens = new Set(text.split(/\s+/).filter(Boolean));
  return (
    <div className="sf">
      <div className="sf-input-wrap">
        <Icon name="filter" size={14} className="icon sf-icon" />
        <input
          className="sf-input"
          value={text}
          onChange={(e) => setText(() => e.target.value)}
          placeholder="Filter: -live minbr:256 …"
          aria-label="Filter results"
          spellCheck={false}
          autoComplete="off"
        />
        {text && (
          <button type="button" className="sf-clear" aria-label="Clear filter" onClick={() => setText(() => '')}>
            <Icon name="x" size={12} />
          </button>
        )}
      </div>
      <FilterHelp />
      <div className="sf-toggles" role="group" aria-label="Quick filters">
        {TOGGLES.map((t) => (
          <Tip key={t.token} label={t.hint}>
            <button type="button" className={`sf-toggle${tokens.has(t.token) ? ' on' : ''}`} aria-pressed={tokens.has(t.token)} onClick={() => setText((x) => toggleToken(x, t.token))}>
              {t.label}
            </button>
          </Tip>
        ))}
        <button type="button" className={`sf-toggle${showLocked ? ' on' : ''}`} aria-pressed={showLocked} onClick={() => setShowLocked(!showLocked)}>
          Locked
        </button>
        <button type="button" className={`sf-toggle${!hideBlacklisted ? ' on' : ''}`} aria-pressed={!hideBlacklisted} onClick={() => setHideBlacklisted(!hideBlacklisted)}>
          Blacklisted
        </button>
      </div>
    </div>
  );
}

/* ---------------------------------------------------------------- rows */

interface RowApi {
  selected: Set<FileKey>;
  toggle: (keys: FileKey[], on: boolean) => void;
  download: (username: string, files: SearchFile[]) => void;
  downloadRemoteFolder: (username: string, folder: string) => Promise<void>;
}

const stop = (e: MouseEvent | KeyboardEvent) => e.stopPropagation();

function Source({ r, alsoFrom }: { r: SearchResponse; alsoFrom: number }) {
  return (
    <span className="cell cell-src">
      <span className="cell-main src-user" onClick={stop}>
        <User name={r.username} />
        {alsoFrom > 0 && (
          <Tip label={`The same folder is shared by ${plural(alsoFrom, 'other user')}. Downloads use them as fallback sources.`}>
            <span className="src-more">+{alsoFrom}</span>
          </Tip>
        )}
      </span>
      <span className="cell-sub num">
        {r.hasFreeUploadSlot ? (
          <span className="src-free">free slot</span>
        ) : (
          <span className="src-queue">{r.queueLength ? `queue ${r.queueLength.toLocaleString()}` : 'no free slot'}</span>
        )}
        <span className="sep">·</span>
        {formatSpeed(r.uploadSpeed)}
      </span>
    </span>
  );
}

const Tracks = memo(function Tracks({ folder, api: h }: { folder: FolderResult; api: RowApi }) {
  const user = folder.response.username;
  const keys = folder.locked ? [] : folder.files.map((f) => fileKey(user, f.filename));
  const selCount = keys.filter((k) => h.selected.has(k)).length;
  const [busy, setBusy] = useState(false);

  return (
    <div className="tracks">
      <ul>
        {folder.files.map((f, i) => {
          const k = fileKey(user, f.filename);
          const on = h.selected.has(k);
          return (
            <li key={f.filename} className={`track${on ? ' on' : ''}`}>
              <label className="track-check">
                <input type="checkbox" checked={on} disabled={folder.locked} onChange={(e) => h.toggle([k], e.target.checked)} aria-label={`Select ${basename(f.filename)}`} />
                <span className="track-n num">{i + 1}</span>
              </label>
              <span className="track-name" title={f.filename}>
                {basename(f.filename)}
              </span>
              <span className="track-q num">{formatQuality(f)}</span>
              <span className="track-len num">{f.length ? formatDuration(f.length) : ''}</span>
              <span className="track-size num">{formatBytes(f.size)}</span>
              {folder.locked ? (
                <span />
              ) : (
                <IconButton icon="download" label="Download file" size="sm" className="track-dl" onClick={() => h.download(user, [f])} />
              )}
            </li>
          );
        })}
      </ul>
      <div className="tracks-foot">
        {folder.locked ? (
          <span className="faint">This folder is locked: {user} only shares it with friends.</span>
        ) : (
          <>
            <Button size="sm" variant="primary" icon="download" onClick={() => h.download(user, folder.files)}>
              Download {plural(folder.files.length, 'file')}
            </Button>
            {selCount > 0 && selCount < keys.length && (
              <Button size="sm" variant="subtle" onClick={() => h.toggle(keys, false)}>
                Clear {selCount} selected
              </Button>
            )}
            <Tip label="Browse the remote folder and queue everything in it, including files this search didn't match">
              <Button
                size="sm"
                variant="ghost"
                icon="folder"
                busy={busy}
                onClick={async () => {
                  setBusy(true);
                  try {
                    await h.downloadRemoteFolder(user, folder.path);
                  } finally {
                    setBusy(false);
                  }
                }}
              >
                Whole folder
              </Button>
            </Tip>
            <span className="spacer" />
            <span className="tracks-path mono" title={folder.path}>
              {folder.path}
            </span>
            <Link to={routes.browse(user)} className="btn btn-ghost btn-sm">
              Browse {user}
            </Link>
          </>
        )}
      </div>
    </div>
  );
});

const FolderRow = memo(function FolderRow({
  folder,
  open,
  onToggle,
  api: h,
  selCount,
}: {
  folder: FolderResult;
  open: boolean;
  onToggle: (key: string) => void;
  api: RowApi;
  selCount: number;
}) {
  const f = folder;
  const tracks = f.audioCount || f.files.length;
  return (
    <li className={`res${open ? ' open' : ''}${f.locked ? ' locked' : ''}`}>
      <div
        className="res-row"
        role="button"
        tabIndex={0}
        aria-expanded={open}
        data-key={f.key}
        onClick={() => onToggle(f.key)}
        onKeyDown={(e) => {
          if (e.target !== e.currentTarget) return;
          if (e.key === 'Enter' || e.key === ' ') {
            e.preventDefault();
            onToggle(f.key);
          } else if (e.key === 'd' && !f.locked) {
            e.preventDefault();
            h.download(f.response.username, f.files);
          }
        }}
      >
        <span className="cell cell-title">
          <span className="cell-main res-name">
            {f.locked && <Icon name="lock" size={13} className="icon res-lock" />}
            <span className="truncate" title={f.path}>
              {f.name}
            </span>
            {selCount > 0 && <span className="res-sel num">{selCount} selected</span>}
          </span>
          <span className="cell-sub truncate">{f.parent || ' '}</span>
        </span>
        <span className="res-meta">
          <span className="cell cell-fmt">
            <span className={`cell-main${f.lossless ? ' fmt-lossless' : ''}`}>{f.format || '—'}</span>
            <span className="cell-sub num">{f.quality || ' '}</span>
          </span>
          <span className="cell cell-tracks">
            <span className="cell-main num">{plural(tracks, f.audioCount ? 'track' : 'file')}</span>
            <span className="cell-sub num">{f.duration ? formatLong(f.duration) : ' '}</span>
          </span>
          <span className="cell cell-size">
            <span className="cell-main num">{formatBytes(f.size)}</span>
            <span className="cell-sub">&nbsp;</span>
          </span>
          <Source r={f.response} alsoFrom={f.alsoFrom} />
        </span>
        <span className="cell cell-act" onClick={stop}>
          {!f.locked && <IconButton icon="download" label={`Download ${plural(f.files.length, 'file')}`} kbd="D" className="res-dl" onClick={() => h.download(f.response.username, f.files)} />}
        </span>
      </div>
      {open && <Tracks folder={f} api={h} />}
    </li>
  );
});

function formatLong(seconds: number): string {
  const m = Math.round(seconds / 60);
  return m >= 60 ? `${Math.floor(m / 60)} h ${m % 60} min` : `${m} min`;
}

/* ---------------------------------------------------------------- results */

const COLUMNS: { key: SortKey; label: string; className: string }[] = [
  { key: 'best', label: 'Best match', className: 'cell-title' },
  { key: 'quality', label: 'Format', className: 'cell-fmt' },
  { key: 'tracks', label: 'Tracks', className: 'cell-tracks' },
  { key: 'size', label: 'Size', className: 'cell-size' },
  { key: 'speed', label: 'Source', className: 'cell-src' },
];

function ColumnHeads({ sort, setSort }: { sort: SortKey; setSort: (k: SortKey) => void }) {
  return (
    <div className="res-head" role="toolbar" aria-label="Sort results">
      {COLUMNS.map((c) => (
        <button key={c.key} type="button" className={`res-head-btn ${c.className}${sort === c.key ? ' on' : ''}`} aria-pressed={sort === c.key} onClick={() => setSort(c.key)}>
          {c.label}
          {sort === c.key && <Icon name="arrowDown" size={11} />}
        </button>
      ))}
      <span className="cell-act" />
    </div>
  );
}

function StatusLine({ search, users, files, filtered }: { search: Search; users: number; files: number; filtered: boolean }) {
  const live = search.state === 'inProgress';
  return (
    <div className="sstatus">
      <h1 className="sstatus-q">{search.query}</h1>
      <span className="sstatus-n num">
        {filtered ? (
          <>
            {files.toLocaleString()} of {search.fileCount.toLocaleString()} files · {users.toLocaleString()} of {search.responseCount.toLocaleString()} users
          </>
        ) : (
          <>
            {search.fileCount.toLocaleString()} files · {search.responseCount.toLocaleString()} users
          </>
        )}
      </span>
      <span className="spacer" />
      {live ? (
        <>
          <span className="sstatus-live">Searching</span>
          <Button
            size="sm"
            variant="subtle"
            icon="stop"
            onClick={() =>
              attempt(async () => {
                const s = await api.stopSearch(search.id);
                store.set((st) => ({ searches: { ...st.searches, [s.id]: s } }));
              })
            }
          >
            Stop
          </Button>
        </>
      ) : (
        <>
          <span className="faint small">{search.state === 'cancelled' ? 'Stopped' : 'Finished'} {formatRelative(search.endedAt ?? search.startedAt)}</span>
          <Button size="sm" variant="subtle" icon="refresh" onClick={() => startSearch(search.query).catch((e) => toastError(e, 'Search failed'))}>
            Search again
          </Button>
        </>
      )}
    </div>
  );
}

function Results({ search }: { search: Search }) {
  const { data: responses, loading, error } = useResponses(search);
  const [filterText, setFilterText] = useState<string>(() => load('searchFilter', ''));
  const [hideBlacklisted, setHideBlacklisted] = useState<boolean>(() => load('hideBlacklisted', true));
  const [showLocked, setShowLocked] = useState<boolean>(() => load('showLocked', false));
  const [sort, setSort] = useState<SortKey>(() => load('searchSort', 'best' as SortKey));
  const [openKeys, setOpenKeys] = useState<Set<string>>(new Set());
  const [selected, setSelected] = useState<Set<FileKey>>(new Set());
  const [limit, setLimit] = useState(PAGE);
  const [destination, setDestination] = useState('');
  const [queuing, setQueuing] = useState(false);
  const blacklist = useStore((s) => s.blacklist);
  const destinations = useDestinations();
  const listRef = useRef<HTMLUListElement>(null);

  useEffect(() => save('searchFilter', filterText), [filterText]);
  useEffect(() => save('hideBlacklisted', hideBlacklisted), [hideBlacklisted]);
  useEffect(() => save('showLocked', showLocked), [showLocked]);
  useEffect(() => save('searchSort', sort), [sort]);
  useEffect(() => {
    setSelected(new Set());
    setOpenKeys(new Set());
    setLimit(PAGE);
  }, [search.id]);

  const filter = useMemo(() => parseFilter(filterText), [filterText]);
  const hideUsers = hideBlacklisted ? blacklist : undefined;
  const groups = useMemo(() => groupResponses(responses, filter, { hideUsers, showLocked }), [responses, filter, hideUsers, showLocked]);
  const folders = useMemo(() => toFolders(groups, search.query), [groups, search.query]);
  const sorted = useMemo(() => (sort === 'best' ? folders : sortFolders(folders, sort)), [folders, sort]);
  const shownFiles = useMemo(() => groups.reduce((n, g) => n + g.fileCount, 0), [groups]);

  const fileIndex = useMemo(() => {
    const m = new Map<FileKey, { username: string; file: SearchFile }>();
    for (const r of responses) for (const f of r.files) m.set(fileKey(r.username, f.filename), { username: r.username, file: f });
    return m;
  }, [responses]);

  const queue = useCallback(
    async (items: { username: string; file: { filename: string; size: number } }[]) => {
      if (!items.length) return;
      setQueuing(true);
      try {
        const payload = items.map(({ username, file }) => {
          const sources = findAlternates(responses, username, file, hideUsers);
          return { username, filename: file.filename, size: file.size, ...(sources.length ? { sources } : {}) };
        });
        const res = await api.download(payload, destination || undefined);
        upsertTransfers(res);
        const multi = payload.filter((p) => p.sources).length;
        toast(`Queued ${plural(res.length, 'file')}`, { type: 'success', description: multi ? `${multi} with fallback sources` : undefined });
      } catch (e) {
        toastError(e, 'Could not queue download');
      } finally {
        setQueuing(false);
      }
    },
    [responses, hideUsers, destination],
  );

  const rowApi: RowApi = useMemo(
    () => ({
      selected,
      toggle: (keys, on) =>
        setSelected((prev) => {
          const next = new Set(prev);
          keys.forEach((k) => (on ? next.add(k) : next.delete(k)));
          return next;
        }),
      download: (username, files) => void queue(files.map((file) => ({ username, file }))),
      downloadRemoteFolder: async (username, folder) => {
        try {
          const dirs = await api.userDirectory(username, folder);
          const items = dirs.flatMap((d) =>
            d.files.map((f) => ({ username, file: { filename: f.filename.includes('\\') ? f.filename : `${d.name}\\${f.filename}`, size: f.size } })),
          );
          if (!items.length) {
            toast('Folder is empty', { type: 'info' });
            return;
          }
          await queue(items);
        } catch (e) {
          toastError(e, 'Could not browse folder');
        }
      },
    }),
    [selected, queue],
  );

  const onToggle = useCallback(
    (key: string) =>
      setOpenKeys((prev) => {
        const next = new Set(prev);
        if (next.has(key)) next.delete(key);
        else next.add(key);
        return next;
      }),
    [],
  );

  // Selected-file counts per folder, for the row badge.
  const selByFolder = useMemo(() => {
    const m = new Map<string, number>();
    if (!selected.size) return m;
    for (const f of folders) {
      if (f.locked) continue;
      let n = 0;
      for (const file of f.files) if (selected.has(fileKey(f.response.username, file.filename))) n++;
      if (n) m.set(f.key, n);
    }
    return m;
  }, [folders, selected]);

  // j/k and arrow keys move between rows.
  const onListKey = (e: KeyboardEvent<HTMLUListElement>) => {
    const keys: Record<string, number> = { ArrowDown: 1, j: 1, ArrowUp: -1, k: -1 };
    const step = keys[e.key];
    if (!step || !(e.target instanceof HTMLElement) || !e.target.classList.contains('res-row')) return;
    const rows = [...(listRef.current?.querySelectorAll<HTMLElement>('.res-row') ?? [])];
    const next = rows[rows.indexOf(e.target) + step];
    if (next) {
      e.preventDefault();
      next.focus();
      next.scrollIntoView({ block: 'nearest' });
    }
  };

  const selItems = useMemo(() => [...selected].map((k) => fileIndex.get(k)).filter((x): x is { username: string; file: SearchFile } => !!x), [selected, fileIndex]);
  const selSize = selItems.reduce((n, i) => n + i.file.size, 0);
  const filtered = groups.length !== search.responseCount || shownFiles !== search.fileCount;
  const live = search.state === 'inProgress';

  let body;
  if (error) body = <ErrorNote error={error} />;
  else if (!sorted.length) {
    body =
      (loading || live) && !responses.length ? (
        <div className="snote">
          <p className="snote-title">Waiting for peers…</p>
          <p>Results arrive as other users answer. Most come in within the first ten seconds.</p>
        </div>
      ) : responses.length ? (
        <div className="snote">
          <p className="snote-title">Nothing matches these filters</p>
          <p>{plural(responses.length, 'user')} answered, but the filters hide every result.</p>
          <Button size="sm" variant="subtle" onClick={() => setFilterText('')}>
            Clear filters
          </Button>
        </div>
      ) : (
        <div className="snote">
          <p className="snote-title">No results</p>
          <p>Nobody answered this query. Try fewer words, or drop words like “the” and punctuation.</p>
        </div>
      );
  } else {
    body = (
      <>
        <ColumnHeads sort={sort} setSort={setSort} />
        <ul className="res-list" ref={listRef} onKeyDown={onListKey}>
          {sorted.slice(0, limit).map((f) => (
            <FolderRow key={f.key} folder={f} open={openKeys.has(f.key)} onToggle={onToggle} api={rowApi} selCount={selByFolder.get(f.key) ?? 0} />
          ))}
        </ul>
        {sorted.length > limit && (
          <div className="res-more">
            <Button variant="subtle" onClick={() => setLimit((l) => l + PAGE)}>
              Show {Math.min(PAGE, sorted.length - limit)} more folders
            </Button>
            <span className="faint small num">{(sorted.length - limit).toLocaleString()} not shown</span>
          </div>
        )}
        <p className="res-keys faint small">
          <Kbd>J</Kbd> <Kbd>K</Kbd> move · <Kbd>↵</Kbd> open · <Kbd>D</Kbd> download
        </p>
      </>
    );
  }

  return (
    <div className="results">
      <StatusLine search={search} users={groups.length} files={shownFiles} filtered={filtered} />
      <FilterBar
        text={filterText}
        setText={(fn) => setFilterText(fn)}
        showLocked={showLocked}
        setShowLocked={setShowLocked}
        hideBlacklisted={hideBlacklisted}
        setHideBlacklisted={setHideBlacklisted}
      />
      {body}
      {selected.size > 0 && (
        <div className="selbar" role="region" aria-label="Selection">
          <span className="medium">{plural(selItems.length, 'file')} selected</span>
          <span className="faint num">{formatBytes(selSize)}</span>
          <span className="spacer" />
          {destinations.length > 0 && (
            <Select
              ariaLabel="Destination"
              value={destination}
              onChange={setDestination}
              options={[{ value: '', label: 'Default folder' }, ...destinations.map((d) => ({ value: d.name, label: d.name }))]}
            />
          )}
          <Button size="sm" variant="ghost" onClick={() => setSelected(new Set())}>
            Clear
          </Button>
          <Button
            size="sm"
            variant="primary"
            icon="download"
            busy={queuing}
            onClick={async () => {
              await queue(selItems);
              setSelected(new Set());
            }}
          >
            Download
          </Button>
        </div>
      )}
    </div>
  );
}

/* ---------------------------------------------------------------- landing */

const norm = (q: string) => q.trim().toLowerCase();

/** Pre-search quick filters: they edit the saved results filter, so results open filtered. */
function HomeFilters() {
  const [text, setText] = useState<string>(() => load('searchFilter', ''));
  const tokens = new Set(text.split(/\s+/).filter(Boolean));
  const extra = [...tokens].filter((t) => !TOGGLES.some((x) => x.token === t));
  const set = (v: string) => {
    setText(v);
    save('searchFilter', v);
  };
  return (
    <div className="home-filters" role="group" aria-label="Filter results">
      {TOGGLES.map((t) => (
        <Tip key={t.token} label={t.hint}>
          <button type="button" className={`sf-toggle${tokens.has(t.token) ? ' on' : ''}`} aria-pressed={tokens.has(t.token)} onClick={() => set(toggleToken(text, t.token))}>
            {t.label}
          </button>
        </Tip>
      ))}
      {extra.length > 0 && (
        <button type="button" className="home-extra mono" title="Saved filter. Click to clear." onClick={() => set(text.split(/\s+/).filter((t) => !extra.includes(t)).join(' '))}>
          <span>{extra.join(' ')}</span>
          <Icon name="x" size={12} />
        </button>
      )}
    </div>
  );
}

function Landing({ list }: { list: Search[] }) {
  // One row per query; the newest search represents it.
  const recent = useMemo(() => {
    const byQuery = new Map<string, Search[]>();
    for (const s of list) {
      const k = norm(s.query);
      const g = byQuery.get(k);
      if (g) g.push(s);
      else byQuery.set(k, [s]);
    }
    return [...byQuery.values()].slice(0, 8);
  }, [list]);

  const rerun = (q: string) => void startSearch(q).catch((e) => toastError(e, 'Search failed'));
  const remove = async (group: Search[]) => {
    for (const s of group) await removeSearch(s.id);
  };

  return (
    <div className="home">
      <SearchBar autoFocus home />
      <HomeFilters />
      {recent.length > 0 ? (
        <section className="home-recent" aria-labelledby="home-recent-h">
          <div className="home-head">
            <h2 id="home-recent-h" className="home-h">
              Recent
            </h2>
            <ClearAll className="home-clear" />
          </div>
          <ul className="home-list">
            {recent.map((group) => {
              const s = group[0];
              return (
                <li key={s.id} className="home-row">
                  <Link to={routes.search(s.id)} className="home-q">
                    {s.state === 'inProgress' && <span className="stab-live" aria-label="searching" />}
                    <span className="home-q-text">{s.query}</span>
                  </Link>
                  <span className="home-meta num">
                    {s.fileCount.toLocaleString()} files · {s.responseCount.toLocaleString()} users
                  </span>
                  <span className="home-t num" title={new Date(s.startedAt).toLocaleString()}>
                    {formatRelative(s.startedAt)}
                  </span>
                  <span className="home-acts">
                    <IconButton icon="refresh" label="Search again" onClick={() => rerun(s.query)} />
                    <IconButton icon="x" label="Remove" onClick={() => void remove(group)} />
                  </span>
                </li>
              );
            })}
          </ul>
        </section>
      ) : (
        <p className="home-empty">Results are grouped by folder, so whole albums are one click away.</p>
      )}
    </div>
  );
}

/* ---------------------------------------------------------------- page */

export function SearchPage({ searchId }: { searchId?: string }) {
  const search = useStore((s) => (searchId ? s.searches[searchId] : undefined));
  const loadedAny = useStore((s) => Object.keys(s.searches).length > 0);
  const list = useSearchList();

  // Deep link to a search not in the store yet.
  useEffect(() => {
    if (searchId && !search) {
      api
        .getSearch(searchId)
        .then((s) => store.set((st) => ({ searches: { ...st.searches, [s.id]: s } })))
        .catch(() => {});
    }
  }, [searchId, search]);

  useEffect(() => {
    if (!loadedAny) void loadSearches().catch(() => {});
  }, [loadedAny]);

  return (
    <div className={`page search-page${searchId ? '' : ' is-home'}`}>
      {searchId ? (
        <>
          <SearchBar autoFocus={false} />
          <SearchTabs list={list} activeId={searchId} />
          {search ? <Results search={search} /> : <div className="snote"><p className="snote-title">Loading search…</p></div>}
        </>
      ) : (
        <Landing list={list} />
      )}
    </div>
  );
}
