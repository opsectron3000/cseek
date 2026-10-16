import { memo, useEffect, useMemo, useState } from 'react';
import { api } from '../api/client';
import type { Transfer, TransferDirection, TransferState } from '../api/types';
import { loadTransfers, removeTransferLocal, upsertTransfers, useStore } from '../store/app';
import { enqueue, playNext, playNow } from '../store/player';
import { formatBytes, formatRelative, formatSpeed, plural } from '../lib/format';
import { basename, dirname, splitRemote } from '../lib/paths';
import { canPlayTransfer, transferTrack } from '../lib/tracks';
import { load, save } from '../lib/storage';
import { Icon } from '../components/Icon';
import { attempt, toast } from '../components/Toaster';
import { ActionMenu, Badge, Button, Empty, Ext, extOf, IconButton, PageHeader, Progress, SegmentedControl, Skeleton, User } from '../components/ui';
import './transfers.css';

const FINISHED: TransferState[] = ['succeeded', 'failed', 'cancelled', 'rejected'];
const ACTIVE: TransferState[] = ['requested', 'inProgress'];
const QUEUED: TransferState[] = ['queued', 'remoteQueued'];
const RETRYABLE: TransferState[] = ['failed', 'cancelled', 'rejected'];

const isFinished = (t: Transfer) => FINISHED.includes(t.state);

type StateFilter = 'all' | 'active' | 'queued' | 'succeeded' | 'errored';

const STATE_LABEL: Record<TransferState, string> = {
  queued: 'Queued',
  remoteQueued: 'Queued',
  requested: 'Requested',
  inProgress: 'Transferring',
  succeeded: 'Done',
  failed: 'Failed',
  cancelled: 'Cancelled',
  rejected: 'Rejected',
};

function stateTone(s: TransferState): 'ok' | 'warn' | 'danger' | 'accent' | 'muted' {
  if (s === 'succeeded') return 'ok';
  if (s === 'inProgress' || s === 'requested') return 'accent';
  if (s === 'failed' || s === 'rejected') return 'danger';
  if (s === 'cancelled') return 'muted';
  return 'warn';
}

function matchesFilter(t: Transfer, f: StateFilter): boolean {
  switch (f) {
    case 'all':
      return true;
    case 'active':
      return ACTIVE.includes(t.state);
    case 'queued':
      return QUEUED.includes(t.state);
    case 'succeeded':
      return t.state === 'succeeded';
    case 'errored':
      return t.state === 'failed' || t.state === 'rejected' || t.state === 'cancelled';
  }
}

interface DirGroup {
  dir: string;
  items: Transfer[];
}
interface UserGroup {
  username: string;
  dirs: DirGroup[];
  items: Transfer[];
}

function groupTransfers(list: Transfer[]): UserGroup[] {
  const users = new Map<string, Map<string, Transfer[]>>();
  for (const t of list) {
    let dirs = users.get(t.username);
    if (!dirs) users.set(t.username, (dirs = new Map()));
    const d = dirname(t.filename);
    const arr = dirs.get(d);
    if (arr) arr.push(t);
    else dirs.set(d, [t]);
  }
  const out: UserGroup[] = [];
  for (const [username, dirs] of users) {
    const dirList = [...dirs.entries()].map(([dir, items]) => ({
      dir,
      items: items.sort((a, b) => a.filename.localeCompare(b.filename, undefined, { numeric: true })),
    }));
    dirList.sort((a, b) => a.dir.localeCompare(b.dir));
    out.push({ username, dirs: dirList, items: dirList.flatMap((d) => d.items) });
  }
  // Most recently requested user first.
  const latest = (g: UserGroup) => g.items.reduce((m, t) => (t.requestedAt > m ? t.requestedAt : m), '');
  out.sort((a, b) => latest(b).localeCompare(latest(a)));
  return out;
}

async function cancel(t: Transfer, remove: boolean) {
  await api.cancelTransfer(t.direction, t.id, remove);
  if (remove) removeTransferLocal(t.id);
}

async function bulk(items: Transfer[], fn: (t: Transfer) => Promise<unknown>, label: string) {
  const results = await Promise.allSettled(items.map(fn));
  const failed = results.filter((r) => r.status === 'rejected').length;
  if (failed) toast(`${label}: ${failed} of ${items.length} failed`, { type: 'error' });
}

const retry = async (t: Transfer) => upsertTransfers([await api.retryDownload(t.id)]);
const refreshPos = async (t: Transfer) => upsertTransfers([await api.refreshPosition(t.id)]);

function Sources({ t }: { t: Transfer }) {
  return (
    <ul className="sources">
      {t.sources.map((s) => (
        <li key={`${s.username}${s.filename}`} className="source-row">
          <span className={`dot dot-src-${s.state}`} />
          <User name={s.username} />
          <span className="source-file truncate" title={s.filename}>
            {basename(s.filename)}
          </span>
          <span className="source-state">{s.state}</span>
          <span className="num">{formatBytes(s.bytes)}</span>
        </li>
      ))}
    </ul>
  );
}

/** Status cell: a live bar while moving, otherwise a state badge. */
function Status({ t }: { t: Transfer }) {
  const progress = t.size ? t.bytesTransferred / t.size : 0;
  if (t.state === 'inProgress') {
    return (
      <div className="tstatus tstatus-live">
        <Progress value={progress} live={t.averageSpeed > 0} label="Progress" />
        <span className="tstatus-line">
          <span>{Math.floor(progress * 100)}%</span>
          <span>{formatSpeed(t.averageSpeed)}</span>
        </span>
      </div>
    );
  }
  const queued = QUEUED.includes(t.state);
  return (
    <div className="tstatus">
      <Badge tone={stateTone(t.state)} title={t.stateDetail ?? undefined}>
        {STATE_LABEL[t.state]}
        {queued && t.placeInQueue !== null && <span className="badge-num">#{t.placeInQueue}</span>}
      </Badge>
      {t.stateDetail && !queued && (
        <span className="tstatus-detail truncate" title={t.stateDetail}>
          {t.stateDetail}
        </span>
      )}
      {queued && progress > 0 && <Progress value={progress} tone="muted" label="Partial" />}
    </div>
  );
}

const TransferRow = memo(function TransferRow({ t }: { t: Transfer }) {
  const [showSources, setShowSources] = useState(false);
  const isDl = t.direction === 'download';
  const finished = isFinished(t);
  const playable = canPlayTransfer(t);
  const activeSources = t.sources.filter((s) => s.state === 'active').length;
  const name = basename(t.filename);

  return (
    <li className={`transfer state-${t.state}`}>
      <div className="transfer-name">
        <Ext ext={extOf(name)} />
        <span className="truncate" title={t.filename}>
          {name}
        </span>
        {t.sources.length > 1 && (
          <button type="button" className="sources-toggle" aria-expanded={showSources} onClick={() => setShowSources((s) => !s)}>
            {finished ? `${t.sources.length} sources` : `${activeSources}/${t.sources.length} sources`}
          </button>
        )}
      </div>
      <Status t={t} />
      <span className="transfer-size">
        {t.state === 'succeeded' || !t.bytesTransferred ? formatBytes(t.size) : `${formatBytes(t.bytesTransferred)} / ${formatBytes(t.size)}`}
      </span>
      <span className="transfer-time" title={t.endedAt ?? t.requestedAt}>
        {formatRelative(t.endedAt ?? t.startedAt ?? t.requestedAt)}
      </span>
      <div className="transfer-actions">
        {playable && <IconButton icon="play" filled size="sm" label="Play" onClick={() => playNow(transferTrack(t))} />}
        {isDl && RETRYABLE.includes(t.state) && <IconButton icon="refresh" size="sm" label="Retry" onClick={() => attempt(() => retry(t))} />}
        {!finished && <IconButton icon="x" size="sm" label="Cancel" onClick={() => attempt(() => cancel(t, false))} />}
        <ActionMenu
          actions={[
            { label: 'Play next', icon: 'play', onSelect: () => playNext(transferTrack(t)), hidden: !playable },
            { label: 'Add to queue', icon: 'queue', onSelect: () => enqueue(transferTrack(t)), hidden: !playable },
            { label: 'Refresh queue position', icon: 'clock', onSelect: () => void attempt(() => refreshPos(t)), hidden: !isDl || !QUEUED.includes(t.state) },
            { label: 'Retry', icon: 'refresh', onSelect: () => void attempt(() => retry(t)), hidden: !isDl || !RETRYABLE.includes(t.state) },
            { label: 'Cancel', icon: 'x', onSelect: () => void attempt(() => cancel(t, false)), hidden: finished },
            { label: finished ? 'Remove' : 'Cancel and remove', icon: 'trash', onSelect: () => void attempt(() => cancel(t, true)), danger: true },
          ]}
        />
      </div>
      {showSources && <Sources t={t} />}
    </li>
  );
});

function GroupActions({ items, direction }: { items: Transfer[]; direction: TransferDirection }) {
  const active = items.filter((t) => !isFinished(t));
  const retryable = items.filter((t) => RETRYABLE.includes(t.state));
  const finished = items.filter(isFinished);
  const queued = items.filter((t) => QUEUED.includes(t.state));
  const playable = items.filter(canPlayTransfer);
  return (
    <ActionMenu
      label="Group actions"
      actions={[
        {
          label: `Play all (${playable.length})`,
          icon: 'play',
          onSelect: () => {
            playNow(transferTrack(playable[0]));
            enqueue(playable.slice(1).map(transferTrack));
          },
          hidden: !playable.length,
        },
        { label: `Add all to queue (${playable.length})`, icon: 'queue', onSelect: () => enqueue(playable.map(transferTrack)), hidden: !playable.length },
        { label: `Retry failed (${retryable.length})`, icon: 'refresh', onSelect: () => void bulk(retryable, retry, 'Retry'), hidden: direction === 'upload' || !retryable.length },
        { label: `Refresh positions (${queued.length})`, icon: 'clock', onSelect: () => void bulk(queued, refreshPos, 'Refresh'), hidden: direction === 'upload' || !queued.length },
        { label: `Cancel (${active.length})`, icon: 'x', onSelect: () => void bulk(active, (t) => cancel(t, false), 'Cancel'), hidden: !active.length },
        { label: `Remove finished (${finished.length})`, icon: 'check', onSelect: () => void bulk(finished, (t) => cancel(t, true), 'Remove'), hidden: !finished.length },
        'separator',
        { label: 'Cancel and remove all', icon: 'trash', onSelect: () => void bulk(items, (t) => cancel(t, true), 'Remove'), danger: true },
      ]}
    />
  );
}

function summarize(items: Transfer[]) {
  const size = items.reduce((n, t) => n + t.size, 0);
  const done = items.reduce((n, t) => n + (t.state === 'succeeded' ? t.size : t.bytesTransferred), 0);
  const speed = items.reduce((n, t) => n + (t.state === 'inProgress' ? t.averageSpeed : 0), 0);
  const succeeded = items.filter((t) => t.state === 'succeeded').length;
  const failed = items.filter((t) => t.state === 'failed' || t.state === 'rejected').length;
  return { size, done, speed, succeeded, failed };
}

function DirPath({ dir }: { dir: string }) {
  const parts = splitRemote(dir);
  if (!parts.length) return <span className="faint">(root)</span>;
  const last = parts.pop()!;
  const prefix = parts.slice(-2).join(' / ');
  return (
    <span className="tdir-path truncate" title={dir}>
      {prefix && <span className="faint">{parts.length > 2 ? '… / ' : ''}{prefix} / </span>}
      {last}
    </span>
  );
}

const UserGroupView = memo(function UserGroupView({ g, direction }: { g: UserGroup; direction: TransferDirection }) {
  const [open, setOpen] = useState(true);
  const sum = summarize(g.items);
  const allDone = sum.succeeded === g.items.length;
  return (
    <section className={`tgroup${open ? '' : ' collapsed'}`}>
      <header className="tgroup-head">
        <button type="button" className="tgroup-toggle" aria-expanded={open} aria-label={open ? 'Collapse' : 'Expand'} onClick={() => setOpen((o) => !o)}>
          <Icon name="chevronDown" size={12} />
        </button>
        <User name={g.username} className="tgroup-user" />
        <span className="tgroup-stats">
          <span className={allDone ? 'ok' : undefined}>
            {sum.succeeded}/{g.items.length} done
          </span>
          {sum.failed > 0 && <span className="danger">{sum.failed} failed</span>}
          <span>
            {formatBytes(sum.done)} / {formatBytes(sum.size)}
          </span>
          {sum.speed > 0 && (
            <span className="tgroup-speed">
              <Icon name={direction === 'download' ? 'arrowDown' : 'arrowUp'} size={11} />
              {formatSpeed(sum.speed)}
            </span>
          )}
        </span>
        <span className="tgroup-progress">{!allDone && <Progress value={sum.size ? sum.done / sum.size : 0} live={sum.speed > 0} label="Group progress" />}</span>
        <GroupActions items={g.items} direction={direction} />
      </header>
      {open &&
        g.dirs.map((d) => (
          <div key={d.dir} className="tdir">
            <div className="tdir-head">
              <Icon name="folder" size={13} />
              <DirPath dir={d.dir} />
              <span className="tdir-count">{d.items.length}</span>
              <span className="tdir-actions">
                <GroupActions items={d.items} direction={direction} />
              </span>
            </div>
            <ul className="transfer-list">
              {d.items.map((t) => (
                <TransferRow key={t.id} t={t} />
              ))}
            </ul>
          </div>
        ))}
    </section>
  );
});

function TransfersSkeleton() {
  return (
    <div className="tgroups" aria-busy="true">
      {[0, 1].map((i) => (
        <section key={i} className="tgroup">
          <header className="tgroup-head">
            <Skeleton width={120} height={12} />
          </header>
          <ul className="transfer-list">
            {[62, 48, 70].map((w) => (
              <li key={w} className="transfer">
                <Skeleton width={`${w}%`} height={10} />
              </li>
            ))}
          </ul>
        </section>
      ))}
    </div>
  );
}

export function TransfersPage({ direction }: { direction: TransferDirection }) {
  const transfers = useStore((s) => s.transfers);
  const loaded = useStore((s) => s.transfersLoaded);
  const [filter, setFilter] = useState<StateFilter>(() => load(`transferFilter.${direction}`, 'all'));
  useEffect(() => save(`transferFilter.${direction}`, filter), [filter, direction]);
  useEffect(() => {
    if (!loaded) void loadTransfers().catch(() => {});
  }, [loaded]);

  const all = useMemo(() => Object.values(transfers).filter((t) => t.direction === direction), [transfers, direction]);
  const groups = useMemo(() => groupTransfers(all.filter((t) => matchesFilter(t, filter))), [all, filter]);
  const counts = useMemo(() => {
    const c = { active: 0, queued: 0, succeeded: 0, errored: 0 };
    for (const t of all) {
      if (ACTIVE.includes(t.state)) c.active++;
      else if (QUEUED.includes(t.state)) c.queued++;
      else if (t.state === 'succeeded') c.succeeded++;
      else c.errored++;
    }
    return c;
  }, [all]);
  const speed = useMemo(() => all.reduce((n, t) => n + (t.state === 'inProgress' ? t.averageSpeed : 0), 0), [all]);
  const isDl = direction === 'download';
  const title = isDl ? 'Downloads' : 'Uploads';
  const shown = groups.reduce((n, g) => n + g.items.length, 0);

  const sub = !all.length
    ? isDl
      ? 'Files you queue from search or browse land here.'
      : 'Files other users fetch from your shares.'
    : [
        counts.active ? `${counts.active} transferring at ${formatSpeed(speed)}` : 'Idle',
        counts.queued ? `${counts.queued} queued` : null,
        `${plural(all.length, 'file')} total`,
      ]
        .filter(Boolean)
        .join(' · ');

  return (
    <div className="page transfers-page">
      <PageHeader title={title} sub={sub}>
        <ActionMenu
          label="Clear"
          trigger={
            <>
              <Icon name="check" size={13} />
              Clear
            </>
          }
          actions={[
            {
              label: `Clear succeeded (${counts.succeeded})`,
              icon: 'check',
              disabled: !counts.succeeded,
              onSelect: () => void attempt(() => api.clearTransfers(direction, 'succeeded').then(() => loadTransfers())),
            },
            {
              label: `Clear all finished (${counts.succeeded + counts.errored})`,
              icon: 'trash',
              disabled: !counts.succeeded && !counts.errored,
              onSelect: () => void attempt(() => api.clearTransfers(direction, 'finished').then(() => loadTransfers())),
            },
          ]}
        />
      </PageHeader>
      {all.length > 0 && (
        <div className="toolbar transfers-toolbar">
          <SegmentedControl<StateFilter>
            ariaLabel="Filter by state"
            value={filter}
            onChange={setFilter}
            options={[
              { value: 'all', label: 'All', count: all.length },
              { value: 'active', label: 'Active', count: counts.active },
              { value: 'queued', label: 'Queued', count: counts.queued },
              { value: 'succeeded', label: 'Done', count: counts.succeeded },
              { value: 'errored', label: 'Failed', count: counts.errored },
            ]}
          />
          <span className="transfers-shown">
            {plural(groups.length, 'user')} · {plural(shown, 'file')}
          </span>
        </div>
      )}
      {!loaded ? (
        <TransfersSkeleton />
      ) : groups.length === 0 ? (
        all.length ? (
          <Empty
            icon="filter"
            title="Nothing here"
            action={
              <Button size="sm" onClick={() => setFilter('all')}>
                Show all
              </Button>
            }
          >
            No {title.toLowerCase()} match this filter.
          </Empty>
        ) : (
          <Empty icon={isDl ? 'download' : 'upload'} title={`No ${title.toLowerCase()} yet`}>
            {isDl ? 'Search the network and queue files to download them.' : 'Uploads appear when someone downloads from your shares.'}
          </Empty>
        )
      ) : (
        <div className="tgroups">
          {groups.map((g) => (
            <UserGroupView key={g.username} g={g} direction={direction} />
          ))}
        </div>
      )}
    </div>
  );
}
