import { useEffect, useState, type ReactNode } from 'react';
import { Popover } from '@base-ui/react/popover';
import { api } from '../api/client';
import { logout, useStore } from '../store/app';
import { usePlayer } from '../store/player';
import { formatSpeed } from '../lib/format';
import { usePath } from '../lib/router';
import { setTheme, useTheme, type ThemePref } from '../lib/theme';
import { Icon } from './Icon';
import { Link } from './Link';
import { Button, Count, IconButton, Kbd, SegmentedControl } from './ui';
import { PlayerBar } from './PlayerBar';
import { attempt } from './Toaster';
import { CommandPalette, isMac, openCommandPalette } from './CommandPalette';
import { NAV_GROUPS } from './nav';

function useBadges(): Record<string, { n: number; accent?: boolean }> {
  const dm = useStore((s) => {
    const convs = Object.values(s.conversations);
    return convs.length ? convs.reduce((n, c) => n + c.unread, 0) : (s.app?.unreadMessages ?? 0);
  });
  const roomUnread = useStore((s) => Object.values(s.roomUnread).reduce((n, c) => n + c, 0));
  const activeDl = useStore((s) => s.app?.transfers.activeDownloads ?? 0);
  const activeUl = useStore((s) => s.app?.transfers.activeUploads ?? 0);
  return {
    '/downloads': { n: activeDl },
    '/uploads': { n: activeUl },
    '/chat': { n: dm + roomUnread, accent: dm > 0 },
  };
}

export function BrandMark({ size = 20 }: { size?: number }) {
  return (
    <span className="brand-mark" style={{ width: size, height: size }} aria-hidden="true">
      <svg width={size * 0.62} height={size * 0.62} viewBox="0 0 16 16" fill="none">
        <path d="M10.6 5.2a4 4 0 1 0 0 5.6" stroke="currentColor" strokeWidth="2.1" strokeLinecap="round" />
        <circle cx="12.1" cy="8" r="1.25" fill="currentColor" />
      </svg>
    </span>
  );
}

function Nav({ onNavigate }: { onNavigate?: () => void }) {
  const path = usePath();
  const badges = useBadges();
  return (
    <nav className="nav" aria-label="Main">
      {NAV_GROUPS.map((g) => (
        <div key={g.label} className="nav-group">
          <div className="nav-group-label">{g.label}</div>
          <ul className="nav-list">
            {g.items.map((it) => {
              const active = path === it.to || path.startsWith(`${it.to}/`);
              const b = badges[it.to];
              return (
                <li key={it.to}>
                  <Link
                    to={it.to}
                    className={`nav-item${active ? ' active' : ''}`}
                    aria-current={active ? 'page' : undefined}
                    onClick={onNavigate}
                  >
                    {active && <span className="nav-pill" />}
                    <Icon name={it.icon} size={15} />
                    <span className="nav-label">{it.label}</span>
                    {b && b.n > 0 && (
                      <span className="nav-badge">
                        <Count n={b.n} accent={b.accent} />
                      </span>
                    )}
                  </Link>
                </li>
              );
            })}
          </ul>
        </div>
      ))}
    </nav>
  );
}

const STATE_LABEL: Record<string, string> = {
  loggedIn: 'Online',
  connecting: 'Connecting…',
  disconnected: 'Disconnected',
  loginFailed: 'Login failed',
};

function useConnection() {
  const app = useStore((s) => s.app);
  const connected = useStore((s) => s.connected);
  const server = app?.server;
  const state = !connected ? 'offline' : (server?.state ?? 'disconnected');
  const tone = !connected
    ? 'danger'
    : state === 'loggedIn'
      ? server?.status === 'away'
        ? 'away'
        : 'online'
      : state === 'connecting'
        ? 'away'
        : state === 'loginFailed'
          ? 'danger'
          : 'offline';
  const label = !connected ? 'Daemon unreachable' : state === 'loggedIn' && server?.status === 'away' ? 'Away' : (STATE_LABEL[state] ?? state);
  return { app, server, tone, label };
}

const THEME_OPTIONS: { value: ThemePref; label: string; icon: 'monitor' | 'moon' | 'sun' }[] = [
  { value: 'system', label: 'Auto', icon: 'monitor' },
  { value: 'dark', label: 'Dark', icon: 'moon' },
  { value: 'light', label: 'Light', icon: 'sun' },
];

function ConnectionPopover({ compact }: { compact?: boolean }) {
  const { app, server, tone, label } = useConnection();
  const auth = useStore((s) => s.auth);
  const theme = useTheme();

  return (
    <Popover.Root>
      <Popover.Trigger className="status-trigger" aria-label={`Connection: ${label}`}>
        <span className={`dot dot-${tone}`} />
        {compact ? null : (
          <span className="status-text">
            <span>{server?.username || 'Not signed in'}</span>
            <span>{label}</span>
          </span>
        )}
        {!compact && <Icon name="chevronDown" size={12} />}
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Positioner className="popover-positioner" sideOffset={6} side={compact ? 'bottom' : 'top'} align={compact ? 'end' : 'start'}>
          <Popover.Popup className="popover status-pop">
            <div className="row gap-xs">
              <span className={`dot dot-${tone}`} />
              <Popover.Title className="popover-title grow">{label}</Popover.Title>
              {server?.isPrivileged && <span className="badge badge-accent">Privileged</span>}
            </div>
            <dl className="kv">
              <dt>User</dt>
              <dd>{server?.username ?? '—'}</dd>
              <dt>Server</dt>
              <dd className="mono">{server?.address ?? '—'}</dd>
              {server?.isPrivileged && (
                <>
                  <dt>Privileges</dt>
                  <dd>{Math.round(server.privilegeSecondsLeft / 86400)} days left</dd>
                </>
              )}
              <dt>Listen port</dt>
              <dd className="mono">{server?.listenPort ?? '—'}</dd>
              <dt>Distributed</dt>
              <dd>{app?.distributed.hasParent ? `parent ${app.distributed.parent ?? '?'}` : 'no parent'}</dd>
              <dt>Daemon</dt>
              <dd className="mono">{app?.version ?? '—'}</dd>
              <dt>Web user</dt>
              <dd>{auth.username ?? '—'}</dd>
            </dl>
            {server?.error && <p className="error-note">{server.error}</p>}
            <div className="row gap-xs wrap">
              {server?.state === 'loggedIn' ? (
                <>
                  <Button size="sm" icon="power" onClick={() => attempt(api.disconnect)}>
                    Disconnect
                  </Button>
                  <Button size="sm" icon="clock" onClick={() => attempt(() => api.setStatus(server.status === 'away' ? 'online' : 'away'))}>
                    {server.status === 'away' ? 'Set online' : 'Set away'}
                  </Button>
                </>
              ) : (
                <Button size="sm" variant="primary" icon="power" onClick={() => attempt(api.connect)} busy={server?.state === 'connecting'}>
                  Connect
                </Button>
              )}
            </div>
            <div className="divider" />
            <div className="row between gap-xs">
              <span className="small muted">Theme</span>
              <SegmentedControl value={theme} onChange={setTheme} options={THEME_OPTIONS} ariaLabel="Theme" />
            </div>
            {!auth.authDisabled && (
              <Button size="sm" variant="ghost" icon="logout" onClick={logout} style={{ alignSelf: 'flex-start', marginLeft: -6 }}>
                Sign out of web UI
              </Button>
            )}
          </Popover.Popup>
        </Popover.Positioner>
      </Popover.Portal>
    </Popover.Root>
  );
}

function Speeds() {
  const t = useStore((s) => s.app?.transfers);
  if (!t) return null;
  return (
    <div className="speeds" aria-label="Transfer speeds">
      <span className={`speed${t.downloadSpeed > 0 ? ' live' : ''}`} title="Download speed">
        <Icon name="arrowDown" size={12} />
        {formatSpeed(t.downloadSpeed)}
      </span>
      <span className={`speed${t.uploadSpeed > 0 ? ' live' : ''}`} title="Upload speed">
        <Icon name="arrowUp" size={12} />
        {formatSpeed(t.uploadSpeed)}
      </span>
    </div>
  );
}

export function Layout({ children }: { children: ReactNode }) {
  const [menuOpen, setMenuOpen] = useState(false);
  const path = usePath();
  const hasPlayer = usePlayer((s) => s.index >= 0);
  useEffect(() => setMenuOpen(false), [path]);
  const section = path.split('/')[1] ?? '';

  return (
    <div className={`shell${hasPlayer ? ' has-player' : ''}`}>
      <header className="mobile-bar">
        <IconButton icon="menu" label="Menu" onClick={() => setMenuOpen(true)} aria-expanded={menuOpen} />
        <div className="brand">
          <BrandMark />
          <span className="brand-name">cseek</span>
        </div>
        <IconButton icon="search" label="Command palette" onClick={openCommandPalette} />
        <ConnectionPopover compact />
      </header>
      <div className={`drawer-backdrop${menuOpen ? ' open' : ''}`} onClick={() => setMenuOpen(false)} aria-hidden="true" />
      <aside className={`sidebar${menuOpen ? ' open' : ''}`}>
        <div className="brand">
          <BrandMark />
          <span className="brand-name">cseek</span>
        </div>
        <button type="button" className="cmdk-trigger" onClick={openCommandPalette} aria-label="Open command palette">
          <Icon name="search" size={13} />
          <span>Jump to…</span>
          <span className="row gap-2xs">
            <Kbd>{isMac ? '⌘' : 'Ctrl'}</Kbd>
            <Kbd>K</Kbd>
          </span>
        </button>
        <Nav onNavigate={() => setMenuOpen(false)} />
        <div className="side-foot">
          <Speeds />
          <ConnectionPopover />
        </div>
      </aside>
      <main className="main">
        <div
          key={section}
          className="main-inner"
        >
          {children}
        </div>
      </main>
      {hasPlayer && <PlayerBar />}
      <CommandPalette />
    </div>
  );
}
