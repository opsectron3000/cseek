import { useCallback, useEffect, useMemo, useRef, useState, type DragEvent, type KeyboardEvent as ReactKeyboardEvent } from 'react';
import { api, streamFileUrl } from '../api/client';
import type { FileEntry, FileListing, FileRoot, ShareSummary } from '../api/types';
import { useStore } from '../store/app';
import { enqueue, playNext, playNow } from '../store/player';
import { formatBytes, formatDateTime, formatRelative, plural } from '../lib/format';
import { isPlayable, joinLocal, parentLocal } from '../lib/paths';
import { fileTrack } from '../lib/tracks';
import { navigate } from '../lib/router';
import { ConfirmDialog, Modal } from '../components/Dialogs';
import { Icon, type IconName } from '../components/Icon';
import { attempt, toast, toastError } from '../components/Toaster';
import { ActionMenu, Button, Checkbox, ErrorNote, Ext, extOf, Field, IconButton, Input, Kbd, Progress } from '../components/ui';
import './files.css';

type SortKey = 'name' | 'size' | 'modified';
type Sort = { key: SortKey; desc: boolean };

const ROOT_ICON: Record<FileRoot['kind'], IconName> = {
  storage: 'folder',
  downloads: 'download',
  incomplete: 'clock',
  destination: 'folder',
  share: 'shares',
};

/** `/files/<root>/<path>` with each segment encoded. */
function filesHref(root: string, path = ''): string {
  const segs = path ? path.split('/').map(encodeURIComponent) : [];
  return ['/files', encodeURIComponent(root), ...segs].join('/');
}

function splitRest(rest = ''): { rootId: string; path: string } {
  const clean = rest.replace(/^\/+|\/+$/g, '');
  if (!clean) return { rootId: 'storage', path: '' };
  const i = clean.indexOf('/');
  return i < 0 ? { rootId: clean, path: '' } : { rootId: clean.slice(0, i), path: clean.slice(i + 1) };
}

function sortEntries(list: FileEntry[], sort: Sort): FileEntry[] {
  const by = (a: FileEntry, b: FileEntry) => {
    if (sort.key === 'size') return a.size - b.size;
    if (sort.key === 'modified') return Date.parse(a.modifiedAt) - Date.parse(b.modifiedAt);
    return a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: 'base' });
  };
  return [...list].sort((a, b) => Number(b.isDirectory) - Number(a.isDirectory) || (sort.desc ? -by(a, b) : by(a, b)));
}

const isTyping = (t: EventTarget | null) =>
  t instanceof HTMLElement && (t.isContentEditable || t.closest('input, textarea, select, [role="dialog"], [role="alertdialog"], [role="menu"]') !== null);

/* ------------------------------------------------------------------ */
/* Folder picker (storage folders for sharing, root folders for moving) */
/* ------------------------------------------------------------------ */

interface PickerListing {
  path: string;
  parent: string | null;
  dirs: { name: string; path: string }[];
}

function FolderPicker({
  load,
  path,
  onPath,
  rootLabel,
  disabled,
}: {
  load: (path: string) => Promise<PickerListing>;
  path: string;
  onPath: (path: string) => void;
  /** Label of the empty path. */
  rootLabel: string;
  disabled?: (dir: string) => boolean;
}) {
  const [listing, setListing] = useState<PickerListing | null>(null);
  const [error, setError] = useState<unknown>(null);
  useEffect(() => {
    let live = true;
    setError(null);
    load(path).then(
      (l) => live && setListing(l),
      (e) => live && setError(e),
    );
    return () => {
      live = false;
    };
  }, [load, path]);

  const crumbs = useMemo(() => {
    const p = listing?.path ?? path;
    const segs = p ? p.split('/') : [];
    return [{ label: rootLabel, path: '' }, ...segs.map((s, i) => ({ label: s, path: segs.slice(0, i + 1).join('/') }))];
  }, [listing, path, rootLabel]);

  return (
    <div className="fp">
      <nav className="fp-crumbs" aria-label="Folder">
        {crumbs.map((c, i) => (
          <span key={c.path + i} className="fp-crumb-wrap">
            {i > 0 && <Icon name="chevronRight" size={10} className="icon faint" />}
            <button type="button" className={`fp-crumb${i === crumbs.length - 1 ? ' on' : ''}`} onClick={() => onPath(c.path)}>
              {c.label}
            </button>
          </span>
        ))}
      </nav>
      <ErrorNote error={error} />
      <ul className="fp-list" aria-label="Folders">
        {listing?.parent != null && (
          <li>
            <button type="button" className="fp-row" onClick={() => onPath(listing.parent!)}>
              <Icon name="arrowUp" size={13} className="icon faint" />
              <span className="muted">Up one level</span>
            </button>
          </li>
        )}
        {listing && listing.dirs.length === 0 && <li className="fp-empty">No folders inside</li>}
        {listing?.dirs.map((d) => (
          <li key={d.path}>
            <button type="button" className="fp-row" disabled={disabled?.(d.path)} onClick={() => onPath(d.path)}>
              <Icon name="folder" size={13} className="icon fm-folder" />
              <span className="truncate">{d.name}</span>
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}

function AddShareDialog({ open, onOpenChange, onAdded }: { open: boolean; onOpenChange: (o: boolean) => void; onAdded: (roots: FileRoot[]) => void }) {
  const [path, setPath] = useState('');
  const [alias, setAlias] = useState('');
  const [busy, setBusy] = useState(false);
  const load = useCallback((p: string) => api.hostDirs(p), []);
  useEffect(() => {
    if (open) {
      setPath('');
      setAlias('');
    }
  }, [open]);
  const [resolved, setResolved] = useState('');
  useEffect(() => {
    if (!open) return;
    let live = true;
    api.hostDirs(path).then(
      (l) => live && setResolved(l.path),
      () => live && setResolved(''),
    );
    return () => {
      live = false;
    };
  }, [open, path]);
  const name = resolved.split('/').filter(Boolean).pop() ?? '';

  return (
    <Modal
      open={open}
      onOpenChange={onOpenChange}
      title="Share a folder"
      description="Pick a folder in storage. Everything inside it becomes searchable and downloadable by other users."
      wide
      footer={
        <>
          <Button onClick={() => onOpenChange(false)}>Cancel</Button>
          <Button
            variant="primary"
            icon="shares"
            busy={busy}
            disabled={!resolved}
            onClick={async () => {
              setBusy(true);
              try {
                const roots = await api.addShare({ path: resolved, alias: alias.trim() || undefined });
                toast(`Sharing ${alias.trim() || name}`, { type: 'success', description: 'Scanning it now.' });
                onAdded(roots);
                onOpenChange(false);
              } catch (e) {
                toastError(e, 'Could not share folder');
              } finally {
                setBusy(false);
              }
            }}
          >
            Share this folder
          </Button>
        </>
      }
    >
      <FolderPicker load={load} path={path} onPath={setPath} rootLabel="All files" />
      <Field label="Name others see" hint={`Leave empty to use “${name || 'the folder name'}”.`}>
        {(id) => <Input id={id} inputSize="sm" placeholder={name} value={alias} onChange={(e) => setAlias(e.target.value)} />}
      </Field>
    </Modal>
  );
}

function MoveDialog({
  root,
  items,
  onOpenChange,
  onMoved,
}: {
  root: FileRoot;
  items: FileEntry[];
  onOpenChange: (o: boolean) => void;
  onMoved: () => void;
}) {
  const [dest, setDest] = useState('');
  const [busy, setBusy] = useState(false);
  const open = items.length > 0;
  useEffect(() => {
    if (open) setDest(parentLocal(items[0].path));
  }, [open, items]);
  const load = useCallback(
    async (p: string): Promise<PickerListing> => {
      const l = await api.files(root.id, p);
      return {
        path: l.path,
        parent: l.path ? parentLocal(l.path) : null,
        dirs: l.entries.filter((e) => e.isDirectory).map((e) => ({ name: e.name, path: e.path })),
      };
    },
    [root.id],
  );
  const moving = new Set(items.filter((i) => i.isDirectory).map((i) => i.path));
  const blocked = (p: string) => [...moving].some((m) => p === m || p.startsWith(m + '/'));
  const same = items.every((i) => parentLocal(i.path) === dest);

  return (
    <Modal
      open={open}
      onOpenChange={onOpenChange}
      title={items.length === 1 ? `Move “${items[0].name}”` : `Move ${plural(items.length, 'item')}`}
      wide
      footer={
        <>
          <Button onClick={() => onOpenChange(false)}>Cancel</Button>
          <Button
            variant="primary"
            busy={busy}
            disabled={same || blocked(dest)}
            onClick={async () => {
              setBusy(true);
              let moved = 0;
              for (const it of items) {
                try {
                  await api.moveFile(root.id, it.path, joinLocal(dest, it.name));
                  moved++;
                } catch (e) {
                  toastError(e, `Could not move ${it.name}`);
                }
              }
              setBusy(false);
              if (moved) toast(`Moved ${plural(moved, 'item')}`, { type: 'success' });
              onMoved();
              onOpenChange(false);
            }}
          >
            Move here
          </Button>
        </>
      }
    >
      <FolderPicker load={load} path={dest} onPath={setDest} rootLabel={root.label} disabled={blocked} />
    </Modal>
  );
}

/* ------------------------------------------------------------------ */
/* Sidebar                                                            */
/* ------------------------------------------------------------------ */

function RootLink({ r, on }: { r: FileRoot; on: boolean }) {
  const count = r.kind === 'share' && r.fileCount !== undefined ? r.fileCount.toLocaleString() : null;
  return (
    <a
      href={filesHref(r.id)}
      className={`fm-loc${on ? ' on' : ''}${r.exists ? '' : ' missing'}`}
      aria-current={on ? 'page' : undefined}
      onClick={(e) => {
        if (e.metaKey || e.ctrlKey || e.shiftKey || e.button !== 0) return;
        e.preventDefault();
        navigate(filesHref(r.id));
      }}
    >
      <Icon name={ROOT_ICON[r.kind]} size={14} className="icon fm-loc-icon" />
      <span className="truncate">{r.label}</span>
      {count && <span className="fm-loc-n">{count}</span>}
    </a>
  );
}

function ScanStatus({ shares }: { shares: ShareSummary | null | undefined }) {
  if (!shares) return null;
  return (
    <div className="fm-scan">
      {shares.scanning ? (
        <>
          <div className="fm-scan-row">
            <span>Scanning…</span>
            <span className="mono">{Math.round(shares.scanProgress * 100)}%</span>
          </div>
          <Progress value={shares.scanProgress} live label="Scan progress" />
        </>
      ) : (
        <dl className="fm-totals">
          <div>
            <dt>Files</dt>
            <dd>{shares.fileCount.toLocaleString()}</dd>
          </div>
          <div>
            <dt>Folders</dt>
            <dd>{shares.directoryCount.toLocaleString()}</dd>
          </div>
          <div>
            <dt>Scanned</dt>
            <dd title={formatDateTime(shares.lastScan)}>{shares.lastScan ? formatRelative(shares.lastScan) : 'never'}</dd>
          </div>
        </dl>
      )}
      <Button size="sm" variant="ghost" icon="refresh" disabled={shares.scanning} onClick={() => attempt(api.rescan)}>
        Rescan shares
      </Button>
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Page                                                               */
/* ------------------------------------------------------------------ */

export function FilesPage({ rest }: { rest?: string }) {
  const { rootId, path } = splitRest(rest);
  const liveShares = useStore((s) => s.app?.shares);
  const [roots, setRoots] = useState<FileRoot[] | null>(null);
  const [rootsError, setRootsError] = useState<unknown>(null);
  const [listing, setListing] = useState<FileListing | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(false);
  const [q, setQ] = useState('');
  const [sort, setSort] = useState<Sort>({ key: 'name', desc: false });
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [cursor, setCursor] = useState(-1);
  const [renaming, setRenaming] = useState<{ path: string; value: string } | null>(null);
  const [deleting, setDeleting] = useState<FileEntry[]>([]);
  const [moving, setMoving] = useState<FileEntry[]>([]);
  const [mkdirOpen, setMkdirOpen] = useState(false);
  const [newName, setNewName] = useState('');
  const [addShareOpen, setAddShareOpen] = useState(false);
  const [unsharing, setUnsharing] = useState<FileRoot | null>(null);
  const [dragging, setDragging] = useState(false);
  const [uploading, setUploading] = useState<string | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const filterInput = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const dragDepth = useRef(0);

  const loadRoots = useCallback(() => {
    api.fileRoots().then(
      (r) => {
        setRoots(r);
        setRootsError(null);
      },
      setRootsError,
    );
  }, []);
  // Share counts change after each scan; refetch roots when one finishes.
  const scanKey = `${liveShares?.lastScan ?? ''}|${liveShares?.directories.length ?? ''}|${liveShares?.scanning ?? ''}`;
  useEffect(loadRoots, [loadRoots, scanKey]);

  const root = roots?.find((r) => r.id === rootId) ?? null;

  const load = useCallback(
    (signal?: AbortSignal) => {
      setLoading(true);
      setError(null);
      api.files(rootId, path, signal).then(
        (l) => {
          setListing(l);
          setLoading(false);
        },
        (e) => {
          if (e instanceof DOMException && e.name === 'AbortError') return;
          setListing(null);
          setError(e);
          setLoading(false);
        },
      );
    },
    [rootId, path],
  );
  useEffect(() => {
    const ac = new AbortController();
    load(ac.signal);
    setSelected(new Set());
    setCursor(-1);
    setRenaming(null);
    setQ('');
    return () => ac.abort();
  }, [load]);

  const fresh = listing && listing.root === rootId && listing.path === path ? listing : null;
  const entries = useMemo(() => {
    const ql = q.trim().toLowerCase();
    const list = (fresh?.entries ?? []).filter((e) => !ql || e.name.toLowerCase().includes(ql));
    return sortEntries(list, sort);
  }, [fresh, q, sort]);
  const playable = entries.filter((e) => !e.isDirectory && isPlayable(e.name));
  const picked = entries.filter((e) => selected.has(e.path));
  const allPicked = entries.length > 0 && picked.length === entries.length;

  const go = (p: string) => navigate(filesHref(rootId, p));
  const open = (e: FileEntry) => {
    if (e.isDirectory) go(e.path);
    else if (isPlayable(e.name)) playNow(fileTrack(e, rootId));
    else window.open(streamFileUrl(e.path, rootId), '_blank', 'noopener');
  };
  const download = (list: FileEntry[]) => {
    for (const e of list.filter((x) => !x.isDirectory)) {
      const a = document.createElement('a');
      a.href = streamFileUrl(e.path, rootId, true);
      a.download = e.name;
      document.body.appendChild(a);
      a.click();
      a.remove();
    }
  };
  const toggle = (p: string, on?: boolean) =>
    setSelected((s) => {
      const next = new Set(s);
      if (on ?? !next.has(p)) next.add(p);
      else next.delete(p);
      return next;
    });

  const upload = async (files: File[]) => {
    if (!files.length) return;
    setUploading(files.length === 1 ? files[0].name : plural(files.length, 'file'));
    try {
      const saved = await api.upload(rootId, path, files);
      toast(`Uploaded ${plural(saved.length, 'file')}`, { type: 'success' });
    } catch (e) {
      toastError(e, 'Upload failed');
    } finally {
      setUploading(null);
      load();
    }
  };

  const committing = useRef(false);
  const commitRename = async () => {
    const r = renaming;
    if (!r || committing.current) return;
    committing.current = true;
    const name = r.value.trim();
    setRenaming(null);
    try {
      if (name && name !== r.path.split('/').pop()) {
        await attempt(() => api.moveFile(rootId, r.path, joinLocal(parentLocal(r.path), name)), 'Could not rename');
        load();
      }
    } finally {
      committing.current = false;
    }
  };

  // Keyboard: j/k or arrows move, Enter opens, Space selects, Backspace goes up.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey || isTyping(e.target) || renaming) return;
      if (document.querySelector('.dialog')) return;
      const n = entries.length;
      const k = e.key;
      if (k === 'j' || k === 'ArrowDown') {
        if (!n) return;
        e.preventDefault();
        setCursor((c) => Math.min(n - 1, c + 1));
      } else if (k === 'k' || k === 'ArrowUp') {
        if (!n) return;
        e.preventDefault();
        setCursor((c) => Math.max(0, c - 1));
      } else if (k === 'Enter' && cursor >= 0 && cursor < n) {
        e.preventDefault();
        open(entries[cursor]);
      } else if (k === ' ' && cursor >= 0 && cursor < n) {
        e.preventDefault();
        toggle(entries[cursor].path);
      } else if (k === 'Backspace' && path) {
        e.preventDefault();
        go(parentLocal(path));
      } else if (k === 'Delete' && picked.length) {
        e.preventDefault();
        setDeleting(picked);
      } else if (k === 'Escape' && selected.size) {
        setSelected(new Set());
      } else if (k === 'F2' && cursor >= 0 && cursor < n) {
        e.preventDefault();
        setRenaming({ path: entries[cursor].path, value: entries[cursor].name });
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });
  useEffect(() => {
    if (cursor < 0) return;
    listRef.current?.querySelector(`[data-i="${cursor}"]`)?.scrollIntoView({ block: 'nearest' });
  }, [cursor]);

  const onDrag = (e: DragEvent, kind: 'enter' | 'leave' | 'over' | 'drop') => {
    if (!e.dataTransfer.types.includes('Files')) return;
    e.preventDefault();
    if (kind === 'enter') {
      dragDepth.current++;
      setDragging(true);
    } else if (kind === 'leave') {
      dragDepth.current = Math.max(0, dragDepth.current - 1);
      if (!dragDepth.current) setDragging(false);
    } else if (kind === 'drop') {
      dragDepth.current = 0;
      setDragging(false);
      void upload([...e.dataTransfer.files]);
    }
  };

  const shareRoots = roots?.filter((r) => r.kind === 'share') ?? [];
  const locations = roots?.filter((r) => r.kind !== 'share') ?? [];
  const shareHere = (fresh?.shareId && shareRoots.find((r) => r.id === fresh.shareId)) || null;
  const crumbs = path ? path.split('/') : [];
  const nDirs = fresh?.entries.filter((e) => e.isDirectory).length ?? 0;
  const nFiles = (fresh?.entries.length ?? 0) - nDirs;
  const unknownRoot = roots && !root;

  const sortHead = (key: SortKey, label: string, cls: string) => (
    <button
      type="button"
      className={`fm-head-btn ${cls}${sort.key === key ? ' on' : ''}`}
      aria-sort={sort.key === key ? (sort.desc ? 'descending' : 'ascending') : undefined}
      onClick={() => setSort((s) => ({ key, desc: s.key === key ? !s.desc : key !== 'name' }))}
    >
      {label}
      {sort.key === key && <Icon name={sort.desc ? 'arrowDown' : 'arrowUp'} size={10} />}
    </button>
  );

  return (
    <div className="page page-fill files-page">
      <aside className="fm-side" aria-label="Locations">
        <div className="fm-side-sec">
          <h2 className="fm-side-h">Locations</h2>
          <nav className="fm-locs">
            {locations.map((r) => (
              <RootLink key={r.id} r={r} on={r.id === rootId} />
            ))}
          </nav>
        </div>
        <div className="fm-side-sec">
          <div className="fm-side-h-row">
            <h2 className="fm-side-h">Shared</h2>
            <IconButton icon="plus" label="Share a folder" onClick={() => setAddShareOpen(true)} />
          </div>
          <nav className="fm-locs">
            {shareRoots.map((r) => (
              <RootLink key={r.id} r={r} on={r.id === rootId} />
            ))}
            {roots && shareRoots.length === 0 && (
              <button type="button" className="fm-loc fm-loc-add" onClick={() => setAddShareOpen(true)}>
                <Icon name="plus" size={14} className="icon fm-loc-icon" />
                <span>Share a folder</span>
              </button>
            )}
          </nav>
        </div>
        <ErrorNote error={rootsError} />
        <span className="spacer" />
        <ScanStatus shares={liveShares} />
      </aside>

      <section
        className={`fm-main${dragging ? ' dragging' : ''}`}
        onDragEnter={(e) => onDrag(e, 'enter')}
        onDragLeave={(e) => onDrag(e, 'leave')}
        onDragOver={(e) => onDrag(e, 'over')}
        onDrop={(e) => onDrag(e, 'drop')}
      >
        <header className="fm-top">
          <nav aria-label="Breadcrumb" className="fm-crumbs">
            <button type="button" className={`fm-crumb${crumbs.length === 0 ? ' on' : ''}`} onClick={() => go('')}>
              {root && <Icon name={ROOT_ICON[root.kind]} size={14} />}
              {root?.label ?? rootId}
            </button>
            {crumbs.map((c, i) => (
              <span key={i} className="fm-crumb-wrap">
                <Icon name="chevronRight" size={10} className="icon faint" />
                <button type="button" className={`fm-crumb${i === crumbs.length - 1 ? ' on' : ''}`} onClick={() => go(crumbs.slice(0, i + 1).join('/'))}>
                  {c}
                </button>
              </span>
            ))}
            {fresh?.shared && <span className="fm-tag">shared</span>}
          </nav>
        </header>

        {selected.size > 0 ? (
          <div className="fm-bar fm-selbar" role="toolbar" aria-label="Selection">
            <Checkbox checked={allPicked} indeterminate={!allPicked} onChange={(v) => setSelected(v ? new Set(entries.map((e) => e.path)) : new Set())} ariaLabel="Select all" />
            <span className="fm-selcount">
              {picked.length}
              <span className="fm-bl"> {picked.length === 1 ? 'item' : 'items'}</span> selected
            </span>
            <span className="spacer" />
            {picked.some((e) => !e.isDirectory && isPlayable(e.name)) && (
              <Button
                size="sm"
                variant="ghost"
                icon="queue"
                className="fm-hide-sm"
                onClick={() => enqueue(picked.filter((e) => !e.isDirectory && isPlayable(e.name)).map((e) => fileTrack(e, rootId)))}
              >
                Queue
              </Button>
            )}
            {picked.some((e) => !e.isDirectory) && (
              <Button size="sm" variant="ghost" icon="download" aria-label="Download" onClick={() => download(picked)}>
                <span className="fm-bl">Download</span>
              </Button>
            )}
            <Button size="sm" variant="ghost" icon="arrowRight" aria-label="Move to…" onClick={() => setMoving(picked)}>
              <span className="fm-bl">Move to…</span>
            </Button>
            <Button size="sm" variant="danger-ghost" icon="trash" aria-label="Delete" onClick={() => setDeleting(picked)}>
              <span className="fm-bl">Delete</span>
            </Button>
            <IconButton icon="x" label="Clear selection" kbd="Esc" onClick={() => setSelected(new Set())} />
          </div>
        ) : (
          <div className="fm-bar" role="toolbar" aria-label="Folder actions">
            <div className="fm-filter">
              <Input
                ref={filterInput}
                icon="filter"
                inputSize="sm"
                placeholder="Filter this folder"
                aria-label="Filter files"
                value={q}
                onChange={(e) => setQ(e.target.value)}
                onClear={() => setQ('')}
                onKeyDown={(e: ReactKeyboardEvent<HTMLInputElement>) => {
                  if (e.key === 'Escape') {
                    setQ('');
                    e.currentTarget.blur();
                  } else if (e.key === 'ArrowDown' || e.key === 'Enter') {
                    e.preventDefault();
                    e.currentTarget.blur();
                    setCursor(0);
                  }
                }}
              />
            </div>
            <span className="spacer" />
            {playable.length > 0 && (
              <Button
                size="sm"
                variant="primary"
                icon="play"
                onClick={() => {
                  playNow(fileTrack(playable[0], rootId));
                  enqueue(playable.slice(1).map((e) => fileTrack(e, rootId)));
                }}
              >
                Play all
              </Button>
            )}
            <Button size="sm" icon="plus" disabled={!root} onClick={() => (setNewName(''), setMkdirOpen(true))} className="fm-hide-sm">
              New folder
            </Button>
            <Button size="sm" icon="upload" disabled={!root || !!uploading} busy={!!uploading} onClick={() => fileInput.current?.click()}>
              Upload
            </Button>
            <input
              ref={fileInput}
              type="file"
              multiple
              hidden
              onChange={(e) => {
                const files = [...(e.target.files ?? [])];
                e.target.value = '';
                void upload(files);
              }}
            />
            {root && root.exists && (
              <ActionMenu
                label="Folder options"
                actions={[
                  { label: 'New folder…', icon: 'plus', onSelect: () => (setNewName(''), setMkdirOpen(true)) },
                  shareHere
                    ? { label: 'Stop sharing…', icon: 'ban', danger: true, onSelect: () => setUnsharing(shareHere) }
                    : {
                        label: fresh?.shared ? 'Shared via a parent folder' : 'Share this folder',
                        icon: 'shares',
                        disabled: fresh?.shared,
                        onSelect: () =>
                          attempt(async () => {
                            const r = await api.addShare({ root: rootId, path });
                            setRoots(r);
                            toast('Folder shared', { type: 'success', description: 'Scanning it now.' });
                            load();
                          }, 'Could not share folder'),
                      },
                  'separator',
                  { label: 'Refresh', icon: 'refresh', onSelect: () => load() },
                ]}
              />
            )}
          </div>
        )}

        {uploading && (
          <div className="fm-note">
            <Icon name="upload" size={13} /> Uploading {uploading}…
          </div>
        )}
        <ErrorNote error={error} />

        <div className="fm-table" ref={listRef} role="grid" aria-label="Files" aria-busy={loading || undefined}>
          <div className="fm-row fm-head" role="row">
            <span className="fm-check">
              <Checkbox
                checked={allPicked}
                indeterminate={picked.length > 0 && !allPicked}
                disabled={!entries.length}
                onChange={(v) => setSelected(v ? new Set(entries.map((e) => e.path)) : new Set())}
                ariaLabel="Select all"
              />
            </span>
            {sortHead('name', 'Name', 'fm-c-name')}
            {sortHead('size', 'Size', 'fm-c-size')}
            {sortHead('modified', 'Modified', 'fm-c-mod')}
            <span />
          </div>

          {unknownRoot ? (
            <div className="fm-empty">
              <p className="fm-empty-t">That location no longer exists</p>
              <p>It may have been removed from your shares or settings.</p>
              <Button size="sm" onClick={() => navigate('/files')}>
                Go to Downloads
              </Button>
            </div>
          ) : !fresh && !error ? (
            <div className="fm-empty">
              <p className="fm-empty-t">Loading…</p>
            </div>
          ) : fresh && entries.length === 0 ? (
            fresh.entries.length ? (
              <div className="fm-empty">
                <p className="fm-empty-t">Nothing matches “{q}”</p>
                <Button size="sm" variant="ghost" onClick={() => setQ('')}>
                  Clear filter
                </Button>
              </div>
            ) : (
              <div className="fm-empty">
                <p className="fm-empty-t">{root && !root.exists ? 'This folder hasn’t been created yet' : 'This folder is empty'}</p>
                <p>
                  {rootId === 'downloads' && !path
                    ? 'Completed downloads land here. Drop files anywhere on this panel to upload them.'
                    : rootId === 'storage' && !path
                      ? 'Everything cseek keeps lives here: downloads, and any folder you share. Drop files here or use Upload to add some.'
                      : 'Drop files here or use Upload to add some.'}
                </p>
              </div>
            )
          ) : (
            entries.map((e, i) => {
              const on = selected.has(e.path);
              const play = !e.isDirectory && isPlayable(e.name);
              const isRenaming = renaming?.path === e.path;
              return (
                <div
                  key={e.path}
                  data-i={i}
                  role="row"
                  aria-selected={on}
                  className={`fm-row${on ? ' sel' : ''}${cursor === i ? ' cur' : ''}`}
                  onClick={(ev) => {
                    setCursor(i);
                    if (ev.shiftKey || ev.metaKey || ev.ctrlKey) toggle(e.path);
                  }}
                  onDoubleClick={() => !isRenaming && open(e)}
                >
                  <span className="fm-check" onClick={(ev) => ev.stopPropagation()}>
                    <Checkbox checked={on} onChange={(v) => toggle(e.path, v)} ariaLabel={`Select ${e.name}`} />
                  </span>
                  <span className="fm-c-name fm-name">
                    {e.isDirectory ? <Icon name="folder" size={15} className="icon fm-folder" /> : <Ext ext={extOf(e.name)} />}
                    {isRenaming ? (
                      <input
                        className="input input-sm fm-rename"
                        autoFocus
                        aria-label="New name"
                        value={renaming.value}
                        onChange={(ev) => setRenaming({ path: e.path, value: ev.target.value })}
                        onFocus={(ev) => {
                          const dot = e.isDirectory ? -1 : ev.target.value.lastIndexOf('.');
                          ev.target.setSelectionRange(0, dot > 0 ? dot : ev.target.value.length);
                        }}
                        onKeyDown={(ev) => {
                          ev.stopPropagation();
                          if (ev.key === 'Enter') void commitRename();
                          else if (ev.key === 'Escape') setRenaming(null);
                        }}
                        onBlur={() => void commitRename()}
                        onClick={(ev) => ev.stopPropagation()}
                      />
                    ) : e.isDirectory ? (
                      <a
                        className="fm-link truncate"
                        href={filesHref(rootId, e.path)}
                        onClick={(ev) => {
                          if (ev.metaKey || ev.ctrlKey || ev.shiftKey) return;
                          ev.preventDefault();
                          go(e.path);
                        }}
                      >
                        {e.name}
                      </a>
                    ) : (
                      <span className="truncate" title={e.name}>
                        {e.name}
                      </span>
                    )}
                    {e.shared && !fresh?.shared && <span className="fm-tag">shared</span>}
                  </span>
                  <span className="fm-c-size fm-num">{e.isDirectory ? '' : formatBytes(e.size)}</span>
                  <span className="fm-c-mod fm-num" title={formatDateTime(e.modifiedAt)}>
                    {formatRelative(e.modifiedAt)}
                  </span>
                  <span className="fm-actions" onClick={(ev) => ev.stopPropagation()} onDoubleClick={(ev) => ev.stopPropagation()}>
                    {play && <IconButton icon="play" filled label="Play" onClick={() => playNow(fileTrack(e, rootId))} />}
                    {!e.isDirectory && (
                      <IconButton icon="download" label="Download" className="fm-hide-sm" onClick={() => download([e])} />
                    )}
                    <ActionMenu
                      actions={[
                        { label: 'Open', icon: 'folder', onSelect: () => go(e.path), hidden: !e.isDirectory },
                        { label: 'Play next', icon: 'next', onSelect: () => playNext(fileTrack(e, rootId)), hidden: !play },
                        { label: 'Add to queue', icon: 'queue', onSelect: () => enqueue(fileTrack(e, rootId)), hidden: !play },
                        { label: 'Download', icon: 'download', onSelect: () => download([e]), hidden: e.isDirectory },
                        { label: 'Rename', icon: 'enter', onSelect: () => setRenaming({ path: e.path, value: e.name }) },
                        { label: 'Move to…', icon: 'arrowRight', onSelect: () => setMoving([e]) },
                        'separator',
                        { label: 'Delete…', icon: 'trash', danger: true, onSelect: () => setDeleting([e]) },
                      ]}
                    />
                  </span>
                </div>
              );
            })
          )}
        </div>

        {fresh && fresh.entries.length > 0 && (
          <footer className="fm-foot">
            <span>
              {plural(nDirs, 'folder')} · {plural(nFiles, 'file')}
              {q && entries.length !== fresh.entries.length && ` · ${entries.length} shown`}
            </span>
            <span className="fm-keys fm-hide-sm">
              <Kbd>j</Kbd>
              <Kbd>k</Kbd> move <Kbd>↵</Kbd> open <Kbd>␣</Kbd> select <Kbd>⌫</Kbd> up
            </span>
          </footer>
        )}

        {dragging && root && (
          <div className="fm-drop" aria-hidden="true">
            <Icon name="upload" size={20} />
            <span>
              Drop to upload into <strong>{crumbs[crumbs.length - 1] ?? root.label}</strong>
            </span>
          </div>
        )}
      </section>

      <Modal
        open={mkdirOpen}
        onOpenChange={setMkdirOpen}
        title="New folder"
        footer={
          <>
            <Button onClick={() => setMkdirOpen(false)}>Cancel</Button>
            <Button variant="primary" disabled={!newName.trim()} type="submit" form="fm-mkdir">
              Create
            </Button>
          </>
        }
      >
        <form
          id="fm-mkdir"
          onSubmit={async (ev) => {
            ev.preventDefault();
            const name = newName.trim();
            if (!name) return;
            const ok = await attempt(() => api.mkdir(rootId, joinLocal(path, name)).then(() => true), 'Could not create folder');
            if (ok) {
              setMkdirOpen(false);
              load();
              loadRoots();
            }
          }}
        >
          <Input autoFocus placeholder="Folder name" aria-label="Folder name" value={newName} onChange={(e) => setNewName(e.target.value)} />
        </form>
      </Modal>

      {root && <MoveDialog root={root} items={moving} onOpenChange={(o) => !o && setMoving([])} onMoved={() => (setSelected(new Set()), load())} />}

      <AddShareDialog open={addShareOpen} onOpenChange={setAddShareOpen} onAdded={setRoots} />

      <ConfirmDialog
        open={deleting.length > 0}
        onOpenChange={(o) => !o && setDeleting([])}
        title={deleting.length === 1 ? (deleting[0].isDirectory ? 'Delete folder?' : 'Delete file?') : `Delete ${plural(deleting.length, 'item')}?`}
        description={
          deleting.length === 1
            ? `“${deleting[0].name}” will be permanently deleted${deleting[0].isDirectory ? ' with everything inside it' : ''}.`
            : 'These will be permanently deleted, including everything inside any folders.'
        }
        confirmLabel="Delete"
        danger
        onConfirm={async () => {
          const list = deleting;
          let n = 0;
          for (const d of list) {
            try {
              await api.deleteFile(rootId, d.path);
              n++;
            } catch (e) {
              toastError(e, `Could not delete ${d.name}`);
            }
          }
          if (n > 1) toast(`Deleted ${plural(n, 'item')}`, { type: 'success' });
          setSelected(new Set());
          load();
        }}
      />

      <ConfirmDialog
        open={!!unsharing}
        onOpenChange={(o) => !o && setUnsharing(null)}
        title={`Stop sharing “${unsharing?.label}”?`}
        description="Other users will no longer see or download anything in it. Nothing on disk is deleted."
        confirmLabel="Stop sharing"
        danger
        onConfirm={async () => {
          const r = unsharing;
          if (!r) return;
          await attempt(async () => {
            setRoots(await api.removeShare(r.id));
            toast(`Stopped sharing ${r.label}`, { type: 'success' });
            if (r.id === rootId) navigate('/files', { replace: true });
            else load();
          }, 'Could not stop sharing');
        }}
      />
    </div>
  );
}
