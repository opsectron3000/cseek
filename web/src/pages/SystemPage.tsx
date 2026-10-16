import { useEffect, useMemo, useRef, useState, type FormEvent } from 'react';
import { api } from '../api/client';
import type { GuardState, LidarrStatus, LogLevel, Metrics } from '../api/types';
import { loadLogs, useStore } from '../store/app';
import { formatBytes, formatDateTime, formatNumber, formatRelative, formatUptime } from '../lib/format';
import { navigate } from '../lib/router';
import { load, save } from '../lib/storage';
import { Icon, type IconName } from '../components/Icon';
import { attempt, toast } from '../components/Toaster';
import { Badge, Button, Card, Empty, ErrorNote, Field, IconButton, Input, Kpi, PageHeader, Select, Skeleton, Switch, TabPanel, Tabs, User } from '../components/ui';
import { OptionsForm } from './OptionsForm';
import './system.css';

const TABS = ['options', 'metrics', 'logs', 'guard', 'nowplaying', 'integrations'] as const;
type Tab = (typeof TABS)[number];
const TAB_META: Record<Tab, { label: string; icon: IconName }> = {
  options: { label: 'Options', icon: 'sliders' },
  metrics: { label: 'Metrics', icon: 'bolt' },
  logs: { label: 'Logs', icon: 'system' },
  guard: { label: 'Guard', icon: 'lock' },
  nowplaying: { label: 'Now playing', icon: 'music' },
  integrations: { label: 'Integrations', icon: 'link' },
};

function KpiSkeleton({ n }: { n: number }) {
  return (
    <div className="kpis kpis-wide">
      {Array.from({ length: n }, (_, i) => (
        <div key={i} className="kpi">
          <Skeleton width={70} height={10} />
          <Skeleton width={90} height={20} style={{ marginTop: 6 }} />
        </div>
      ))}
    </div>
  );
}

function CopyValue({ value }: { value: string }) {
  return (
    <span className="copy-value">
      <code className="mono selectable truncate">{value}</code>
      <IconButton
        icon="copy"
        label="Copy"
        onClick={() => navigator.clipboard.writeText(value).then(() => toast('Copied', { type: 'success' }), () => toast('Copy failed', { type: 'error' }))}
      />
    </span>
  );
}

function MetricsTab() {
  const [m, setM] = useState<Metrics | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [auto, setAuto] = useState(true);
  const [busy, setBusy] = useState(false);
  const refresh = () => {
    setBusy(true);
    api
      .metrics()
      .then((x) => {
        setM(x);
        setError(null);
      }, setError)
      .finally(() => setBusy(false));
  };
  useEffect(refresh, []);
  useEffect(() => {
    if (!auto) return;
    const t = setInterval(refresh, 5000);
    return () => clearInterval(t);
  }, [auto]);
  const app = useStore((s) => s.app);

  return (
    <div className="stack">
      <div className="toolbar">
        <Switch checked={auto} onChange={setAuto} label="Live" />
        {auto && <span className="live-dot" aria-hidden="true" />}
        <span className="spacer" />
        <a className="small muted ext-link" href="/metrics" target="_blank" rel="noreferrer">
          Prometheus <Icon name="external" size={11} />
        </a>
        <IconButton icon="refresh" label="Refresh" onClick={refresh} disabled={busy} />
      </div>
      <ErrorNote error={error} />
      {!m ? (
        !error && <KpiSkeleton n={8} />
      ) : (
        <div className="kpis kpis-wide">
          <Kpi label="Uptime" value={formatUptime(m.uptimeSeconds)} />
          <Kpi label="Downloaded" value={formatBytes(m.bytesDownloaded)} />
          <Kpi label="Uploaded" value={formatBytes(m.bytesUploaded)} />
          <Kpi label="Downloads" value={formatNumber(m.downloadsSucceeded)} sub={`${formatNumber(m.downloadsFailed)} failed`} />
          <Kpi label="Uploads" value={formatNumber(m.uploadsSucceeded)} sub={`${formatNumber(m.uploadsFailed)} failed`} />
          <Kpi label="Searches sent" value={formatNumber(m.searchesSent)} />
          <Kpi label="Search requests in" value={formatNumber(m.searchRequestsReceived)} sub={`${formatNumber(m.searchResponsesSent)} responses sent`} />
          <Kpi label="Peer connections" value={formatNumber(m.peerConnections)} />
          {app && (
            <>
              <Kpi label="Active transfers" value={`${app.transfers.activeDownloads} / ${app.transfers.activeUploads}`} sub={`down / up · ${app.transfers.queuedUploads} queued uploads`} />
              <Kpi
                label="Distributed"
                value={app.distributed.hasParent ? `level ${app.distributed.branchLevel}` : 'no parent'}
                sub={app.distributed.parent ? `parent ${app.distributed.parent} · ${formatNumber(app.distributed.searchesReceived)} searches` : `${formatNumber(app.distributed.searchesReceived)} searches`}
              />
            </>
          )}
        </div>
      )}
    </div>
  );
}

const LEVELS: LogLevel[] = ['TRACE', 'DEBUG', 'INFO', 'WARN', 'ERROR'];

function LogsTab() {
  const logs = useStore((s) => s.logs);
  const [level, setLevel] = useState<LogLevel>(() => load('logLevel', 'INFO'));
  const [q, setQ] = useState('');
  const [follow, setFollow] = useState(true);
  const [error, setError] = useState<unknown>(null);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    loadLogs().catch(setError);
  }, []);
  useEffect(() => save('logLevel', level), [level]);

  const min = LEVELS.indexOf(level);
  const shown = useMemo(() => {
    const ql = q.toLowerCase();
    return logs.filter((l) => LEVELS.indexOf(l.level) >= min && (!ql || l.message.toLowerCase().includes(ql) || l.target.toLowerCase().includes(ql)));
  }, [logs, min, q]);

  useEffect(() => {
    if (follow && ref.current) ref.current.scrollTop = ref.current.scrollHeight;
  }, [shown, follow]);

  return (
    <div className="stack">
      <div className="toolbar">
        <div className="logs-filter">
          <Input icon="filter" inputSize="sm" placeholder="Filter message or target" aria-label="Filter logs" value={q} onChange={(e) => setQ(e.target.value)} onClear={() => setQ('')} />
        </div>
        <Select<LogLevel> ariaLabel="Minimum level" value={level} onChange={setLevel} options={LEVELS.map((l) => ({ value: l, label: `${l} and above` }))} />
        <Switch checked={follow} onChange={setFollow} label="Follow" />
        <span className="spacer" />
        <span className="small faint num">
          {shown.length.toLocaleString()} / {logs.length.toLocaleString()}
        </span>
      </div>
      <ErrorNote error={error} />
      <div className="logs mono" ref={ref} role="log">
        {shown.map((l, i) => (
          <div key={i} className={`log-line log-${l.level.toLowerCase()}`}>
            <span className="log-time">{new Date(l.timestamp).toLocaleTimeString()}</span>
            <span className="log-level">{l.level}</span>
            <span className="log-target">{l.target}</span>
            <span className="log-msg">{l.message}</span>
          </div>
        ))}
        {shown.length === 0 && (
          <Empty icon="system" title={logs.length ? 'No matching entries' : 'No log entries yet'}>
            {logs.length ? 'Try a lower level or a different filter.' : undefined}
          </Empty>
        )}
      </div>
    </div>
  );
}

function GuardTab() {
  const [g, setG] = useState<GuardState | null>(null);
  const [error, setError] = useState<unknown>(null);
  const refresh = () => api.guard().then(setG, setError);
  useEffect(() => {
    void refresh();
  }, []);
  if (!g) return error ? <ErrorNote error={error} /> : <KpiSkeleton n={2} />;
  const refreshBtn = <IconButton icon="refresh" label="Refresh" onClick={refresh} />;
  return (
    <div className="stack">
      <Card
        title={
          <>
            Bans <span className="faint num">{g.bans.length || ''}</span>
          </>
        }
        actions={refreshBtn}
      >
        {g.bans.length === 0 ? (
          <Empty icon="ban" title="No active bans">
            Users who trip the guard limits show up here.
          </Empty>
        ) : (
          <div className="table-wrap">
            <table className="table">
              <thead>
                <tr>
                  <th>User</th>
                  <th>Reason</th>
                  <th>Until</th>
                  <th aria-label="Actions" />
                </tr>
              </thead>
              <tbody>
                {g.bans.map((b) => (
                  <tr key={b.username}>
                    <td>
                      <User name={b.username} />
                    </td>
                    <td className="small muted">{b.reason}</td>
                    <td className="small nowrap">{b.until ? formatDateTime(b.until) : <Badge tone="danger">permanent</Badge>}</td>
                    <td className="actions">
                      <Button
                        size="xs"
                        variant="ghost"
                        onClick={() =>
                          attempt(async () => {
                            await api.unban(b.username);
                            setG((x) => (x ? { ...x, bans: x.bans.filter((y) => y.username !== b.username) } : x));
                          })
                        }
                      >
                        Unban
                      </Button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Card>
      <Card
        title={
          <>
            Violations <span className="faint num">{g.violations.length || ''}</span>
          </>
        }
      >
        {g.violations.length === 0 ? (
          <Empty icon="eye" title="No violations recorded" />
        ) : (
          <div className="table-wrap">
            <table className="table">
              <thead>
                <tr>
                  <th>User</th>
                  <th>Kind</th>
                  <th className="num">Count</th>
                  <th>Last</th>
                </tr>
              </thead>
              <tbody>
                {[...g.violations]
                  .sort((a, b) => b.lastAt.localeCompare(a.lastAt))
                  .map((v) => (
                    <tr key={`${v.username}${v.kind}`}>
                      <td>
                        <User name={v.username} />
                      </td>
                      <td>
                        <Badge outline>{v.kind}</Badge>
                      </td>
                      <td className="num">{v.count}</td>
                      <td className="small nowrap" title={formatDateTime(v.lastAt)}>
                        {formatRelative(v.lastAt)}
                      </td>
                    </tr>
                  ))}
              </tbody>
            </table>
          </div>
        )}
      </Card>
    </div>
  );
}

function NowPlayingTab() {
  const current = useStore((s) => s.app?.nowPlaying ?? null);
  const [artist, setArtist] = useState('');
  const [title, setTitle] = useState('');
  const [album, setAlbum] = useState('');
  const [busy, setBusy] = useState(false);
  const origin = location.origin;

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    const res = await attempt(() => api.setNowPlaying({ artist: artist.trim(), title: title.trim(), ...(album.trim() ? { album: album.trim() } : {}) }));
    setBusy(false);
    if (res) {
      toast('Now playing updated', { type: 'success' });
      setArtist('');
      setTitle('');
      setAlbum('');
    }
  };

  return (
    <div className="stack">
      <Card title="Current">
        {current ? (
          <div className="np-current">
            <span className="np-art" aria-hidden="true">
              <Icon name="music" size={16} />
            </span>
            <div className="grow" style={{ minWidth: 0 }}>
              <div className="medium truncate">
                {current.artist} — {current.title}
              </div>
              <div className="small muted truncate">
                {current.album ? `${current.album} · ` : ''}via {current.source} · {formatRelative(current.startedAt)}
              </div>
            </div>
            <Button size="sm" variant="ghost" icon="x" onClick={() => attempt(api.clearNowPlaying)}>
              Clear
            </Button>
          </div>
        ) : (
          <Empty icon="music" title="Nothing playing">
            Set it below, or let Jellyfin / Plex report it via webhook.
          </Empty>
        )}
      </Card>
      <Card title="Set manually">
        <form className="np-form" onSubmit={submit}>
          <Field label="Artist">{(id) => <Input id={id} inputSize="sm" value={artist} onChange={(e) => setArtist(e.target.value)} required />}</Field>
          <Field label="Title">{(id) => <Input id={id} inputSize="sm" value={title} onChange={(e) => setTitle(e.target.value)} required />}</Field>
          <Field label="Album" hint="Optional">
            {(id) => <Input id={id} inputSize="sm" value={album} onChange={(e) => setAlbum(e.target.value)} />}
          </Field>
          <Button type="submit" size="sm" variant="primary" busy={busy} disabled={!artist.trim() || !title.trim()}>
            Set
          </Button>
        </form>
      </Card>
      <Card title="Webhooks">
        <p className="small muted">
          Point media servers at these URLs. Authenticate with an API key (<code>X-API-Key</code> header) from Options → Web.
        </p>
        <dl className="kv kv-webhooks">
          <dt>
            Generic <Badge outline>PUT</Badge>
          </dt>
          <dd>
            <CopyValue value={`${origin}/api/nowplaying`} />
          </dd>
          <dt>
            Jellyfin <Badge outline>POST</Badge>
          </dt>
          <dd>
            <CopyValue value={`${origin}/api/nowplaying/webhook/jellyfin`} />
          </dd>
          <dt>
            Plex <Badge outline>POST</Badge>
          </dt>
          <dd>
            <CopyValue value={`${origin}/api/nowplaying/webhook/plex`} />
          </dd>
        </dl>
        <p className="small faint">
          Generic body: <code>{'{ "artist", "title", "album"? }'}</code>
        </p>
      </Card>
    </div>
  );
}

function IntegrationsTab() {
  const [s, setS] = useState<LidarrStatus | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [busy, setBusy] = useState(false);
  const refresh = () => api.lidarrStatus().then(setS, setError);
  useEffect(() => {
    void refresh();
  }, []);
  return (
    <Card
      title="Lidarr"
      actions={s ? s.enabled ? <Badge tone="ok" icon="check">Enabled</Badge> : <Badge>Disabled</Badge> : undefined}
    >
      <ErrorNote error={error} />
      {!s ? (
        !error && <Skeleton width="60%" height={14} />
      ) : (
        <div className="stack">
          <dl className="kv">
            <dt>Last sync</dt>
            <dd title={formatDateTime(s.lastSync)}>{formatRelative(s.lastSync)}</dd>
            <dt>Last added</dt>
            <dd className="num">{s.lastAdded}</dd>
            {s.lastError && (
              <>
                <dt>Last error</dt>
                <dd className="error-text">{s.lastError}</dd>
              </>
            )}
          </dl>
          <div className="row gap-xs">
            <Button
              size="sm"
              variant="primary"
              icon="refresh"
              busy={busy}
              disabled={!s.enabled}
              onClick={async () => {
                setBusy(true);
                const r = await attempt(api.lidarrSync);
                setBusy(false);
                if (r) toast(`Lidarr sync: ${r.added} added to wishlist`, { type: 'success' });
                void refresh();
              }}
            >
              Sync now
            </Button>
            <Button size="sm" variant="ghost" icon="sliders" onClick={() => navigate('/system/options')}>
              Configure
            </Button>
          </div>
        </div>
      )}
    </Card>
  );
}

export function SystemPage({ tab }: { tab?: string }) {
  const current: Tab = (TABS as readonly string[]).includes(tab ?? '') ? (tab as Tab) : 'options';
  const version = useStore((s) => s.app?.version);
  const errors = useStore((s) => s.logs.reduce((n, l) => n + (l.level === 'ERROR' ? 1 : 0), 0));
  return (
    <div className="page">
      <PageHeader title="System" sub={version ? `cseekd ${version}` : undefined} />
      <Tabs<Tab>
        value={current}
        onChange={(v) => navigate(`/system/${v}`, { replace: true })}
        ariaLabel="System sections"
        tabs={TABS.map((t) => ({ value: t, label: TAB_META[t].label, icon: TAB_META[t].icon, count: t === 'logs' && errors ? errors : undefined }))}
      >
        <TabPanel value="options">
          <OptionsForm />
        </TabPanel>
        <TabPanel value="metrics">
          <MetricsTab />
        </TabPanel>
        <TabPanel value="logs">
          <LogsTab />
        </TabPanel>
        <TabPanel value="guard">
          <GuardTab />
        </TabPanel>
        <TabPanel value="nowplaying">
          <NowPlayingTab />
        </TabPanel>
        <TabPanel value="integrations">
          <IntegrationsTab />
        </TabPanel>
      </Tabs>
    </div>
  );
}
