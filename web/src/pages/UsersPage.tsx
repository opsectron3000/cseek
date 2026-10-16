import { useEffect, useState, type FormEvent, type ReactNode } from 'react';
import { Tabs } from '@base-ui/react/tabs';
import { Popover } from '@base-ui/react/popover';
import { api, userPictureUrl } from '../api/client';
import type { Groups, Interests, UserInfo, UserProfile, UserStats, UserStatusInfo } from '../api/types';
import { noteUserGroup } from '../store/app';
import { closeBrowse, openBrowse, useBrowse } from '../store/browse';
import { formatBytes, formatSpeed } from '../lib/format';
import { navigate, routes, useQuery } from '../lib/router';
import { load, save } from '../lib/storage';
import { BrowseView } from '../components/BrowseView';
import { Link } from '../components/Link';
import { attempt, toast } from '../components/Toaster';
import { Avatar, Badge, Button, countryName, Empty, Flag, Input, Kpi, PageHeader, Select, Skeleton, Spinner, Textarea, useCountry, UserName } from '../components/ui';
import { Icon } from '../components/Icon';
import './users.css';

/** Country, which opens the user's IP address on click (looked up on demand). */
function AddressFact({ username, cc }: { username: string; cc?: string | null }) {
  const [addr, setAddr] = useState<{ ip: string; port: number } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const lookup = () => {
    setError(null);
    api.userAddress(username).then(setAddr, (e: unknown) => setError(e instanceof Error ? e.message : String(e)));
  };
  useEffect(() => {
    setAddr(null);
    setError(null);
  }, [username]);
  const copy = (text: string) =>
    navigator.clipboard.writeText(text).then(
      () => toast('Copied', { type: 'success', description: text }),
      () => toast('Could not copy', { type: 'error' }),
    );
  return (
    <Popover.Root onOpenChange={(open) => open && !addr && lookup()}>
      <Popover.Trigger className="profile-fact profile-addr" title="Show IP address">
        {cc ? (
          <>
            <Flag cc={cc} tip={false} />
            {countryName(cc)}
          </>
        ) : (
          <>
            <Icon name="globe" size={13} />
            Address
          </>
        )}
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Positioner className="popover-positioner" sideOffset={8} align="start">
          <Popover.Popup className="popover addr-pop">
            <div className="addr-label">{cc ? countryName(cc) : 'Address'}</div>
            {addr ? (
              <div className="addr-row">
                <span className="mono addr-ip">{addr.ip}</span>
                <span className="mono faint">:{addr.port}</span>
                <span className="spacer" />
                <Button size="sm" variant="subtle" icon="copy" onClick={() => copy(addr.ip)}>
                  Copy IP
                </Button>
              </div>
            ) : error ? (
              <div className="addr-row">
                <span className="faint">{error}</span>
                <span className="spacer" />
                <Button size="sm" variant="ghost" onClick={lookup}>
                  Retry
                </Button>
              </div>
            ) : (
              <div className="addr-row faint">Asking the server…</div>
            )}
          </Popover.Popup>
        </Popover.Positioner>
      </Popover.Portal>
    </Popover.Root>
  );
}

type Loadable<T> = { data: T | null; error: string | null; loading: boolean };
const initial = { data: null, error: null, loading: true };

function useLoad<T>(fn: () => Promise<T>, deps: unknown[]): [Loadable<T>, (d: T) => void] {
  const [s, setS] = useState<Loadable<T>>(initial);
  useEffect(() => {
    let alive = true;
    setS(initial);
    fn().then(
      (data) => alive && setS({ data, error: null, loading: false }),
      (e: unknown) => alive && setS({ data: null, error: e instanceof Error ? e.message : String(e), loading: false }),
    );
    return () => {
      alive = false;
    };
  }, deps);
  return [s, (data: T) => setS({ data, error: null, loading: false })];
}

function Section<T>({ title, state, children, className }: { title: string; state: Loadable<T>; children: (d: T) => ReactNode; className?: string }) {
  return (
    <section className={`card${className ? ` ${className}` : ''}`}>
      <h2 className="card-title">{title}</h2>
      {state.loading ? (
        <div className="stack-sm" aria-hidden="true">
          <Skeleton width="70%" height={11} />
          <Skeleton width="45%" height={11} />
          <Skeleton width="58%" height={11} />
        </div>
      ) : state.error ? (
        <p className="small faint">{state.error}</p>
      ) : state.data !== null ? (
        children(state.data)
      ) : null}
    </section>
  );
}

let groupsCache: Groups | null = null;

function GroupSelect({ profile, onChange }: { profile: UserProfile; onChange: (p: UserProfile) => void }) {
  const [groups, setGroups] = useState<Groups | null>(groupsCache);
  useEffect(() => {
    if (!groupsCache) api.groups().then((g) => setGroups((groupsCache = g)), () => {});
  }, []);
  const names = new Set<string>(['leechers', 'blacklisted', ...(groups?.builtin ?? []), ...(groups?.custom.map((g) => g.name) ?? [])]);
  if (profile.group) names.add(profile.group);
  const options = [{ value: '', label: 'Default' }, ...[...names].map((n) => ({ value: n, label: n.charAt(0).toUpperCase() + n.slice(1) }))];
  return (
    <Select
      ariaLabel="User group"
      value={profile.group ?? ''}
      options={options}
      onChange={(v) =>
        void attempt(async () => {
          const p = await api.setUserGroup(profile.username, v || null);
          onChange(p);
          noteUserGroup(p.username, p.group);
          if (p.group === 'blacklisted') toast(`${p.username} blacklisted`, { description: 'Their transfers were cancelled.' });
        })
      }
    />
  );
}

function NoteEditor({ profile, onChange }: { profile: UserProfile; onChange: (p: UserProfile) => void }) {
  const [note, setNote] = useState(profile.note);
  const [busy, setBusy] = useState(false);
  useEffect(() => setNote(profile.note), [profile.note]);
  const dirty = note !== profile.note;
  return (
    <div className="stack-sm">
      <Textarea rows={3} value={note} onChange={(e) => setNote(e.target.value)} aria-label="Private note" placeholder="Private note about this user" />
      <div className="row end gap-xs">
        {dirty && (
          <Button size="sm" variant="ghost" onClick={() => setNote(profile.note)}>
            Revert
          </Button>
        )}
        <Button
          size="sm"
          variant="primary"
          disabled={!dirty}
          busy={busy}
          onClick={async () => {
            setBusy(true);
            const p = await attempt(() => api.setUserNote(profile.username, note));
            if (p) onChange(p);
            setBusy(false);
          }}
        >
          Save note
        </Button>
      </div>
    </div>
  );
}

function Picture({ username, has }: { username: string; has: boolean }) {
  const [failed, setFailed] = useState(false);
  if (!has || failed) return <Avatar name={username} size={56} />;
  return <img className="profile-pic" src={userPictureUrl(username)} alt={`${username}'s picture`} onError={() => setFailed(true)} />;
}

function Profile({ username }: { username: string }) {
  const [info] = useLoad<UserInfo>(() => api.userInfo(username), [username]);
  const [status] = useLoad<UserStatusInfo>(() => api.userStatus(username), [username]);
  const [stats] = useLoad<UserStats>(() => api.userStats(username), [username]);
  const [interests] = useLoad<Interests>(() => api.userInterests(username), [username]);
  const [profile, setProfile] = useLoad<UserProfile>(() => api.userProfile(username), [username]);

  const rep = profile.data?.reputation;
  const cc = useCountry(username);
  const st = status.data?.status;
  return (
    <div className="profile">
      <div className="profile-head">
        <Picture username={username} has={!!info.data?.hasPicture} />
        <div className="profile-id">
          <div className="row gap-xs wrap">
            <h2 className="profile-name" title={username}>
              <UserName name={username} full />
            </h2>
            {status.data?.isPrivileged && (
              <Badge tone="accent" icon="star">
                privileged
              </Badge>
            )}
            {profile.data?.group && <Badge tone={profile.data.group === 'blacklisted' ? 'danger' : 'muted'}>{profile.data.group}</Badge>}
            {rep && (
              <Badge tone={rep.score > 10 ? 'ok' : rep.score < -10 ? 'danger' : 'muted'} title="Local reputation">
                rep {rep.score > 0 ? '+' : ''}
                {rep.score}
              </Badge>
            )}
          </div>
          <div className="profile-facts">
            {status.loading ? (
              <Skeleton width={60} height={10} />
            ) : (
              st && (
                <span className="profile-fact">
                  <span className={`dot dot-${st}`} />
                  {st}
                </span>
              )
            )}
            <AddressFact username={username} cc={cc} />
            {stats.data && (
              <span className="profile-fact num">
                {stats.data.fileCount.toLocaleString()} files · {formatSpeed(stats.data.averageSpeed)}
              </span>
            )}
          </div>
        </div>
        <div className="profile-actions">
          <Button size="sm" variant="primary" icon="folder" onClick={() => navigate(routes.browse(username))}>
            Browse shares
          </Button>
          <Button size="sm" icon="message" onClick={() => navigate(routes.dm(username))}>
            Message
          </Button>
        </div>
      </div>

      <div className="grid-2">
        <Section title="Description" state={info}>
          {(d) => (
            <>
              <p className="pre-wrap profile-desc">{d.description || <span className="faint">No description.</span>}</p>
              <dl className="kv">
                <dt>Upload slots</dt>
                <dd className="num">{d.uploadSlots}</dd>
                <dt>Queue</dt>
                <dd className="num">{d.queueLength}</dd>
                <dt>Free slot</dt>
                <dd>
                  <span className={`slot ${d.hasFreeUploadSlot ? 'slot-free' : 'slot-busy'}`}>
                    <span className={`dot ${d.hasFreeUploadSlot ? 'dot-online' : 'dot-away'}`} />
                    {d.hasFreeUploadSlot ? 'Available' : 'Queued'}
                  </span>
                </dd>
              </dl>
            </>
          )}
        </Section>
        <Section title="Stats" state={stats}>
          {(s) => (
            <div className="kpis">
              <Kpi label="Speed" value={formatSpeed(s.averageSpeed)} />
              <Kpi label="Files" value={s.fileCount.toLocaleString()} />
              <Kpi label="Folders" value={s.directoryCount.toLocaleString()} />
              <Kpi label="Uploads" value={s.uploadCount.toLocaleString()} />
            </div>
          )}
        </Section>
        <Section title="Local profile" state={profile}>
          {(p) => (
            <div className="stack">
              <label className="field">
                <span className="field-label">Group</span>
                <GroupSelect profile={p} onChange={setProfile} />
              </label>
              <div className="field">
                <span className="field-label">Note</span>
                <NoteEditor profile={p} onChange={setProfile} />
              </div>
            </div>
          )}
        </Section>
        <Section title="Reputation" state={profile}>
          {(p) => (
            <dl className="kv">
              <dt>Score</dt>
              <dd>{p.reputation.score}</dd>
              <dt>Downloads ok / failed</dt>
              <dd>
                {p.reputation.downloadsSucceeded} / {p.reputation.downloadsFailed}
              </dd>
              <dt>Uploads to them</dt>
              <dd>{p.reputation.uploadsSucceeded}</dd>
              <dt>Downloaded from</dt>
              <dd className="mono">{formatBytes(p.reputation.bytesDownloaded)}</dd>
              <dt>Uploaded to</dt>
              <dd className="mono">{formatBytes(p.reputation.bytesUploaded)}</dd>
              <dt>Violations</dt>
              <dd>{p.reputation.violations}</dd>
            </dl>
          )}
        </Section>
        <Section title="Interests" state={interests}>
          {(i) => (
            <div className="grid-2 tight">
              <div>
                <h3 className="interest-label">Likes</h3>
                {i.likes.length ? (
                  <ul className="chips">
                    {i.likes.map((x) => (
                      <li key={x} className="chip">
                        {x}
                      </li>
                    ))}
                  </ul>
                ) : (
                  <p className="small faint">None</p>
                )}
              </div>
              <div>
                <h3 className="interest-label">Hates</h3>
                {i.hates.length ? (
                  <ul className="chips">
                    {i.hates.map((x) => (
                      <li key={x} className="chip">
                        {x}
                      </li>
                    ))}
                  </ul>
                ) : (
                  <p className="small faint">None</p>
                )}
              </div>
            </div>
          )}
        </Section>
      </div>
    </div>
  );
}

export function UsersPage({ username }: { username?: string }) {
  const query = useQuery();
  const wantBrowse = query.get('tab') === 'browse';
  const sessions = useBrowse((s) => s.sessions);
  const [lookup, setLookup] = useState('');
  const [recent, setRecent] = useState<string[]>(() => load('recentUsers', []));
  const [tab, setTab] = useState<string>('profile');

  useEffect(() => {
    if (!username) return;
    setRecent((r) => {
      const next = [username, ...r.filter((x) => x !== username)].slice(0, 12);
      save('recentUsers', next);
      return next;
    });
    if (wantBrowse) {
      openBrowse(username);
      setTab(`b:${username}`);
    } else setTab('profile');
  }, [username, wantBrowse]);

  // If the active browse tab was closed, fall back.
  useEffect(() => {
    if (tab.startsWith('b:') && !sessions.some((s) => `b:${s.username}` === tab)) setTab('profile');
  }, [sessions, tab]);

  const submit = (e: FormEvent) => {
    e.preventDefault();
    const v = lookup.trim();
    if (!v) return;
    setLookup('');
    navigate(routes.user(v));
  };

  return (
    <div className="page">
      <PageHeader title="Users" sub={username ? undefined : 'Profiles, notes and shares'}>
        <form onSubmit={submit} className="row gap-xs">
          <Input icon="user" inputSize="sm" placeholder="Username" aria-label="Look up user" value={lookup} onChange={(e) => setLookup(e.target.value)} />
          <Button size="sm" type="submit" disabled={!lookup.trim()}>
            Look up
          </Button>
          <Button
            size="sm"
            icon="folder"
            disabled={!lookup.trim()}
            onClick={() => {
              const v = lookup.trim();
              setLookup('');
              navigate(routes.browse(v));
            }}
          >
            Browse
          </Button>
        </form>
      </PageHeader>

      {!username && sessions.length === 0 ? (
        <Empty icon="user" title="Look someone up">
          Enter a username above to see their profile or browse their shares.
          {recent.length > 0 && (
            <span className="recent-users">
              {recent.map((r) => (
                <Link key={r} to={routes.user(r)} className="recent-user">
                  <Avatar name={r} size={18} />
                  {r}
                </Link>
              ))}
            </span>
          )}
        </Empty>
      ) : (
        <Tabs.Root value={tab} onValueChange={(v) => setTab(String(v))} className="tabs">
          <Tabs.List className="tab-list" aria-label="User views">
            {username && (
              <Tabs.Tab value="profile" className="tab">
                <Icon name="user" size={13} />
                <span title={username}>
                  <UserName name={username} />
                </span>
              </Tabs.Tab>
            )}
            {sessions.map((s) => (
              <Tabs.Tab key={s.username} value={`b:${s.username}`} className="tab tab-closable">
                {s.loading ? <Spinner size={11} /> : <Icon name="folder" size={13} />}
                <span title={s.username}>
                  <UserName name={s.username} />
                </span>
                <span
                  role="button"
                  tabIndex={-1}
                  className="tab-x"
                  aria-label={`Close ${s.username}`}
                  onClick={(e) => {
                    e.stopPropagation();
                    closeBrowse(s.username);
                  }}
                >
                  <Icon name="x" size={10} />
                </span>
              </Tabs.Tab>
            ))}
            <Tabs.Indicator className="tab-indicator" />
          </Tabs.List>
          {username && (
            <Tabs.Panel value="profile" className="tab-panel">
              <Profile key={username} username={username} />
            </Tabs.Panel>
          )}
          {sessions.map((s) => (
            <Tabs.Panel key={s.username} value={`b:${s.username}`} className="tab-panel" keepMounted>
              <BrowseView session={s} />
            </Tabs.Panel>
          ))}
        </Tabs.Root>
      )}
    </div>
  );
}
