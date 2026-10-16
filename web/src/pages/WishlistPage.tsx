import { useEffect, useMemo, useState, type FormEvent } from 'react';
import { api } from '../api/client';
import type { WishlistFilter, WishlistItem } from '../api/types';
import { loadWishlist, store, useStore } from '../store/app';
import { formatBytes, formatRelative } from '../lib/format';
import { navigate, routes } from '../lib/router';
import { Link } from '../components/Link';
import { ConfirmDialog, Modal } from '../components/Dialogs';
import { attempt, toast } from '../components/Toaster';
import { Icon } from '../components/Icon';
import { ActionMenu, Badge, Button, Empty, Ext, Field, Input, PageHeader, Skeleton, StringList, Switch } from '../components/ui';
import './wishlist.css';

const MB = 1024 * 1024;

interface Draft {
  query: string;
  enabled: boolean;
  autoDownload: boolean;
  extensions: string[];
  minBitRate: string;
  minSizeMb: string;
  maxSizeMb: string;
}

const toDraft = (w?: WishlistItem): Draft => ({
  query: w?.query ?? '',
  enabled: w?.enabled ?? true,
  autoDownload: w?.autoDownload ?? false,
  extensions: w?.filter.extensions ?? [],
  minBitRate: w?.filter.minBitRate?.toString() ?? '',
  minSizeMb: w?.filter.minSize ? String(+(w.filter.minSize / MB).toFixed(2)) : '',
  maxSizeMb: w?.filter.maxSize ? String(+(w.filter.maxSize / MB).toFixed(2)) : '',
});

const num = (s: string): number | null => (s.trim() === '' || Number.isNaN(Number(s)) ? null : Number(s));

const toFilter = (d: Draft): WishlistFilter => ({
  extensions: d.extensions.map((e) => e.replace(/^\./, '').toLowerCase()),
  minBitRate: num(d.minBitRate),
  minSize: num(d.minSizeMb) !== null ? Math.round(num(d.minSizeMb)! * MB) : null,
  maxSize: num(d.maxSizeMb) !== null ? Math.round(num(d.maxSizeMb)! * MB) : null,
});

function FilterSummary({ f }: { f: WishlistFilter }) {
  const empty = !f.extensions.length && !f.minBitRate && !f.minSize && !f.maxSize;
  if (empty) return <span className="wish-any">Any file</span>;
  return (
    <span className="wish-filter">
      {f.extensions.map((e) => (
        <Ext key={e} ext={e} />
      ))}
      {f.minBitRate ? <Badge outline>≥{f.minBitRate} kbps</Badge> : null}
      {f.minSize ? <Badge outline>≥{formatBytes(f.minSize)}</Badge> : null}
      {f.maxSize ? <Badge outline>≤{formatBytes(f.maxSize)}</Badge> : null}
    </span>
  );
}

function Editor({ item, open, onOpenChange }: { item?: WishlistItem; open: boolean; onOpenChange: (o: boolean) => void }) {
  const [d, setD] = useState<Draft>(toDraft(item));
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (open) setD(toDraft(item));
  }, [open, item]);
  const up = (p: Partial<Draft>) => setD((x) => ({ ...x, ...p }));

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!d.query.trim()) return;
    setBusy(true);
    const saved = await attempt(() =>
      item
        ? api.updateWishlist(item.id, { query: d.query.trim(), enabled: d.enabled, autoDownload: d.autoDownload, filter: toFilter(d) })
        : api.addWishlist({ query: d.query.trim(), autoDownload: d.autoDownload, filter: toFilter(d) }),
    );
    setBusy(false);
    if (saved) {
      store.set((s) => ({ wishlist: { ...s.wishlist, [saved.id]: saved } }));
      onOpenChange(false);
    }
  };

  return (
    <Modal open={open} onOpenChange={onOpenChange} title={item ? 'Edit wishlist item' : 'Add to wishlist'}>
      <form className="stack" onSubmit={submit}>
        <Field label="Query">
          {(id) => <Input id={id} icon="search" autoFocus value={d.query} onChange={(e) => up({ query: e.target.value })} placeholder="artist album" required />}
        </Field>
        <div className="row gap wrap">
          {item && <Switch checked={d.enabled} onChange={(v) => up({ enabled: v })} label="Enabled" />}
          <Switch checked={d.autoDownload} onChange={(v) => up({ autoDownload: v })} label="Auto-download best match" />
        </div>
        <Field label="Extensions" hint="Leave empty to accept any file type.">
          <StringList value={d.extensions} onChange={(v) => up({ extensions: v })} placeholder="flac" ariaLabel="Add extension" />
        </Field>
        <div className="grid-3">
          <Field label="Min bitrate (kbps)">
            {(id) => <Input id={id} type="number" min={0} value={d.minBitRate} onChange={(e) => up({ minBitRate: e.target.value })} />}
          </Field>
          <Field label="Min size (MB)">
            {(id) => <Input id={id} type="number" min={0} step="any" value={d.minSizeMb} onChange={(e) => up({ minSizeMb: e.target.value })} />}
          </Field>
          <Field label="Max size (MB)">
            {(id) => <Input id={id} type="number" min={0} step="any" value={d.maxSizeMb} onChange={(e) => up({ maxSizeMb: e.target.value })} />}
          </Field>
        </div>
        <div className="row end gap-xs">
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button type="submit" variant="primary" busy={busy} disabled={!d.query.trim()}>
            {item ? 'Save' : 'Add'}
          </Button>
        </div>
      </form>
    </Modal>
  );
}

export function WishlistPage() {
  const wishlist = useStore((s) => s.wishlist);
  const [loaded, setLoaded] = useState(false);
  const [editing, setEditing] = useState<WishlistItem | undefined>();
  const [editorOpen, setEditorOpen] = useState(false);
  const [deleting, setDeleting] = useState<WishlistItem | null>(null);
  const [running, setRunning] = useState<string | null>(null);

  useEffect(() => {
    loadWishlist().finally(() => setLoaded(true));
  }, []);

  const items = useMemo(() => Object.values(wishlist).sort((a, b) => b.createdAt.localeCompare(a.createdAt)), [wishlist]);
  const patch = async (w: WishlistItem, p: Partial<WishlistItem>) => {
    const saved = await attempt(() => api.updateWishlist(w.id, p));
    if (saved) store.set((s) => ({ wishlist: { ...s.wishlist, [saved.id]: saved } }));
  };
  const run = async (w: WishlistItem) => {
    setRunning(w.id);
    const saved = await attempt(() => api.runWishlist(w.id));
    setRunning(null);
    if (saved) {
      store.set((s) => ({ wishlist: { ...s.wishlist, [saved.id]: saved } }));
      toast(`Searching “${saved.query}”`);
    }
  };

  const openEditor = (w?: WishlistItem) => {
    setEditing(w);
    setEditorOpen(true);
  };
  const enabled = items.filter((w) => w.enabled).length;
  const auto = items.filter((w) => w.enabled && w.autoDownload).length;

  return (
    <div className="page">
      <PageHeader
        title="Wishlist"
        sub={items.length ? `${enabled} of ${items.length} active · ${auto} auto-downloading` : 'Saved searches that re-run periodically and can grab the best match.'}
      >
        <Button variant="primary" size="sm" icon="plus" onClick={() => openEditor()}>
          Add
        </Button>
      </PageHeader>
      {!loaded && !items.length ? (
        <ul className="wish-list" aria-busy="true">
          {[56, 40, 64].map((w) => (
            <li key={w} className="wish">
              <Skeleton width={`${w}%`} height={12} />
            </li>
          ))}
        </ul>
      ) : items.length === 0 ? (
        <Empty
          icon="wishlist"
          title="Nothing wished for yet"
          action={
            <Button size="sm" variant="subtle" icon="plus" onClick={() => openEditor()}>
              Add a wish
            </Button>
          }
        >
          Wishlist searches run periodically and can auto-download the best match.
        </Empty>
      ) : (
        <ul className="wish-list">
          {items.map((w) => (
            <li key={w.id} className={`wish${w.enabled ? '' : ' off'}`}>
              <Switch checked={w.enabled} onChange={(v) => void patch(w, { enabled: v })} ariaLabel={`Enable ${w.query}`} />
              <div className="wish-main">
                <button type="button" className="wish-query truncate" onClick={() => openEditor(w)} title="Edit">
                  {w.query}
                </button>
                <FilterSummary f={w.filter} />
              </div>
              <div className="wish-stats">
                <span className="wish-stat" title="Matches in last run">
                  <Icon name="search" size={11} />
                  {w.lastMatchCount}
                </span>
                <span className={`wish-stat${w.downloadedCount ? ' ok' : ''}`} title="Downloaded">
                  <Icon name="download" size={11} />
                  {w.downloadedCount}
                </span>
                <span className="wish-run" title={w.lastRunAt ?? undefined}>
                  {w.lastSearchId ? <Link to={routes.search(w.lastSearchId)}>{formatRelative(w.lastRunAt)}</Link> : w.lastRunAt ? formatRelative(w.lastRunAt) : 'never'}
                </span>
              </div>
              <Switch checked={w.autoDownload} onChange={(v) => void patch(w, { autoDownload: v })} label="Auto" ariaLabel={`Auto-download ${w.query}`} />
              <div className="wish-actions">
                <Button size="sm" variant="ghost" icon="refresh" busy={running === w.id} onClick={() => run(w)}>
                  Run
                </Button>
                <ActionMenu
                  actions={[
                    { label: 'Edit', icon: 'sliders', onSelect: () => openEditor(w) },
                    { label: 'Run now', icon: 'refresh', onSelect: () => void run(w) },
                    { label: 'Open last search', icon: 'search', onSelect: () => navigate(routes.search(w.lastSearchId!)), hidden: !w.lastSearchId },
                    'separator',
                    { label: 'Delete', icon: 'trash', danger: true, onSelect: () => setDeleting(w) },
                  ]}
                />
              </div>
            </li>
          ))}
        </ul>
      )}
      <Editor item={editing} open={editorOpen} onOpenChange={setEditorOpen} />
      <ConfirmDialog
        open={!!deleting}
        onOpenChange={(o) => !o && setDeleting(null)}
        title="Delete wishlist item?"
        description={deleting ? `“${deleting.query}” will no longer be searched.` : undefined}
        confirmLabel="Delete"
        danger
        onConfirm={async () => {
          const w = deleting;
          if (!w) return;
          await attempt(async () => {
            await api.deleteWishlist(w.id);
            store.set((s) => {
              const { [w.id]: _gone, ...rest } = s.wishlist;
              return { wishlist: rest };
            });
          });
        }}
      />
    </div>
  );
}
