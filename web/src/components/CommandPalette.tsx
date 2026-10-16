import { useEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type ReactNode } from 'react';
import { Dialog } from '@base-ui/react/dialog';
import { api } from '../api/client';
import { logout, useStore } from '../store/app';
import { joinRoom, startSearch } from '../lib/actions';
import { navigate, routes } from '../lib/router';
import { setTheme, useTheme } from '../lib/theme';
import { Icon, type IconName } from './Icon';
import { Kbd } from './ui';
import { attempt, toastError } from './Toaster';
import { NAV_GROUPS } from './nav';

interface Command {
  id: string;
  group: string;
  label: string;
  icon: IconName;
  hint?: ReactNode;
  /** Extra words to match against. */
  keywords?: string;
  run: () => void | Promise<void>;
}

const OPEN_EVENT = 'cseek:command-palette';
export const openCommandPalette = () => window.dispatchEvent(new Event(OPEN_EVENT));

/** Subsequence fuzzy match; returns a score (higher is better) or -1. */
function score(text: string, q: string): number {
  const t = text.toLowerCase();
  const i = t.indexOf(q);
  if (i === 0) return 100 - t.length * 0.1;
  if (i > 0) return 80 - i - t.length * 0.1;
  let ti = 0;
  let gaps = 0;
  for (const ch of q) {
    const at = t.indexOf(ch, ti);
    if (at < 0) return -1;
    gaps += at - ti;
    ti = at + 1;
  }
  return 40 - gaps;
}

function highlight(label: string, q: string): ReactNode {
  if (!q) return label;
  const i = label.toLowerCase().indexOf(q);
  if (i < 0) return label;
  return (
    <>
      {label.slice(0, i)}
      <mark>{label.slice(i, i + q.length)}</mark>
      {label.slice(i + q.length)}
    </>
  );
}

function useCommands(query: string): Command[] {
  const searches = useStore((s) => s.searches);
  const conversations = useStore((s) => s.conversations);
  const rooms = useStore((s) => s.rooms);
  const server = useStore((s) => s.app?.server);
  const authDisabled = useStore((s) => s.auth.authDisabled);
  const theme = useTheme();
  const q = query.trim();

  return useMemo(() => {
    const go = (to: string) => () => navigate(to);
    const out: Command[] = [];
    if (q) {
      out.push(
        {
          id: 'q:search',
          group: 'Actions',
          label: `Search the network for “${q}”`,
          icon: 'search',
          hint: <Kbd>↵</Kbd>,
          run: () => startSearch(q).catch((e) => toastError(e, 'Search failed')),
        },
        { id: 'q:user', group: 'Actions', label: `Open user ${q}`, icon: 'user', run: go(routes.user(q)) },
        { id: 'q:browse', group: 'Actions', label: `Browse ${q}’s shares`, icon: 'folder', run: go(routes.browse(q)) },
        { id: 'q:dm', group: 'Actions', label: `Message ${q}`, icon: 'message', run: go(routes.dm(q)) },
        {
          id: 'q:room',
          group: 'Actions',
          label: `Join room ${q}`,
          icon: 'hash',
          run: () => attempt(() => joinRoom(q), 'Could not join room').then(() => undefined),
        },
      );
    }
    for (const g of NAV_GROUPS)
      for (const it of g.items)
        out.push({ id: `nav:${it.to}`, group: 'Go to', label: it.label, icon: it.icon, keywords: it.keywords, run: go(it.to) });
    out.push(
      { id: 'nav:options', group: 'Go to', label: 'Options', icon: 'sliders', keywords: 'settings preferences config', run: go('/system/options') },
      { id: 'nav:logs', group: 'Go to', label: 'Logs', icon: 'system', run: go('/system/logs') },
    );

    const recent = Object.values(searches)
      .sort((a, b) => b.startedAt.localeCompare(a.startedAt))
      .slice(0, q ? 20 : 5);
    for (const s of recent)
      out.push({ id: `s:${s.id}`, group: 'Recent searches', label: s.query, icon: 'clock', hint: <span className="cmdk-hint num">{s.fileCount} files</span>, run: go(routes.search(s.id)) });
    const convs = Object.values(conversations)
      .sort((a, b) => (b.lastMessage?.timestamp ?? '').localeCompare(a.lastMessage?.timestamp ?? ''))
      .slice(0, q ? 30 : 4);
    for (const c of convs) out.push({ id: `c:${c.username}`, group: 'Conversations', label: c.username, icon: 'message', run: go(routes.dm(c.username)) });
    for (const r of Object.keys(rooms)) out.push({ id: `r:${r}`, group: 'Rooms', label: r, icon: 'hash', run: go(routes.room(r)) });

    const loggedIn = server?.state === 'loggedIn';
    out.push(
      {
        id: 'theme',
        group: 'Preferences',
        label: theme === 'light' ? 'Use dark theme' : theme === 'dark' ? 'Use system theme' : 'Use light theme',
        icon: theme === 'light' ? 'moon' : theme === 'dark' ? 'monitor' : 'sun',
        keywords: 'theme dark light mode appearance',
        run: () => setTheme(theme === 'light' ? 'dark' : theme === 'dark' ? 'system' : 'light'),
      },
      loggedIn
        ? { id: 'conn', group: 'Preferences', label: 'Disconnect from Soulseek', icon: 'power', keywords: 'offline logout server', run: () => attempt(api.disconnect).then(() => undefined) }
        : { id: 'conn', group: 'Preferences', label: 'Connect to Soulseek', icon: 'power', keywords: 'online login server', run: () => attempt(api.connect).then(() => undefined) },
    );
    if (loggedIn)
      out.push({
        id: 'away',
        group: 'Preferences',
        label: server?.status === 'away' ? 'Set status: online' : 'Set status: away',
        icon: 'clock',
        keywords: 'status away online',
        run: () => attempt(() => api.setStatus(server?.status === 'away' ? 'online' : 'away')).then(() => undefined),
      });
    if (!authDisabled) out.push({ id: 'logout', group: 'Preferences', label: 'Sign out of web UI', icon: 'logout', run: logout });

    if (!q) return out;
    const needle = q.toLowerCase();
    return out
      .map((c) => ({ c, s: c.id.startsWith('q:') ? 1000 - out.indexOf(c) : Math.max(score(c.label, needle), c.keywords ? score(c.keywords, needle) - 10 : -1) }))
      .filter((x) => x.s >= 0)
      .sort((a, b) => b.s - a.s)
      .map((x) => x.c);
  }, [q, searches, conversations, rooms, server?.state, server?.status, theme, authDisabled]);
}

export function CommandPalette() {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [active, setActive] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);
  const commands = useCommands(query);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && !e.altKey && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        setOpen((o) => !o);
      }
    };
    const onOpen = () => setOpen(true);
    window.addEventListener('keydown', onKey);
    window.addEventListener(OPEN_EVENT, onOpen);
    return () => {
      window.removeEventListener('keydown', onKey);
      window.removeEventListener(OPEN_EVENT, onOpen);
    };
  }, []);

  useEffect(() => setActive(0), [query, open]);
  useEffect(() => {
    if (!open) setQuery('');
  }, [open]);
  useEffect(() => {
    listRef.current?.querySelector('[data-active="true"]')?.scrollIntoView({ block: 'nearest' });
  }, [active]);

  const run = (c: Command) => {
    setOpen(false);
    void c.run();
  };

  const onKeyDown = (e: ReactKeyboardEvent) => {
    if (e.key === 'ArrowDown' || (e.ctrlKey && e.key === 'n')) {
      e.preventDefault();
      setActive((a) => Math.min(commands.length - 1, a + 1));
    } else if (e.key === 'ArrowUp' || (e.ctrlKey && e.key === 'p')) {
      e.preventDefault();
      setActive((a) => Math.max(0, a - 1));
    } else if (e.key === 'Enter') {
      e.preventDefault();
      const c = commands[active];
      if (c) run(c);
    }
  };

  // Group in display order while keeping a flat index for keyboard nav.
  const groups: { name: string; items: { c: Command; i: number }[] }[] = [];
  commands.forEach((c, i) => {
    let g = groups.find((x) => x.name === c.group);
    if (!g) groups.push((g = { name: c.group, items: [] }));
    g.items.push({ c, i });
  });
  const q = query.trim().toLowerCase();

  return (
    <Dialog.Root open={open} onOpenChange={(o) => setOpen(o)}>
      <Dialog.Portal>
        <Dialog.Backdrop className="dialog-backdrop" />
        <Dialog.Popup className="cmdk" aria-label="Command palette">
          <div className="cmdk-input-row">
            <Icon name="search" size={16} />
            <input
              className="cmdk-input"
              autoFocus
              placeholder="Search, jump to, or run a command…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={onKeyDown}
              role="combobox"
              aria-expanded="true"
              aria-controls="cmdk-list"
              aria-activedescendant={commands[active] ? `cmdk-${commands[active].id}` : undefined}
            />
            <Kbd>esc</Kbd>
          </div>
          <div className="cmdk-list" id="cmdk-list" role="listbox" ref={listRef}>
            {commands.length === 0 && <div className="cmdk-empty">No matches</div>}
            {groups.map((g) => (
              <div key={g.name} className="cmdk-group" role="group" aria-label={g.name}>
                <div className="cmdk-group-label">{g.name}</div>
                {g.items.map(({ c, i }) => (
                  <div
                    key={c.id}
                    id={`cmdk-${c.id}`}
                    role="option"
                    aria-selected={i === active}
                    data-active={i === active}
                    className="cmdk-item"
                    onMouseMove={() => i !== active && setActive(i)}
                    onClick={() => run(c)}
                  >
                    {i === active && <span className="cmdk-hl" />}
                    <Icon name={c.icon} size={15} />
                    <span className="cmdk-item-label">{highlight(c.label, q)}</span>
                    {c.hint}
                  </div>
                ))}
              </div>
            ))}
          </div>
          <div className="cmdk-foot">
            <span>
              <Kbd>↑</Kbd>
              <Kbd>↓</Kbd> navigate
            </span>
            <span>
              <Kbd>↵</Kbd> run
            </span>
            <span style={{ marginLeft: 'auto' }}>
              <Kbd>{isMac ? '⌘' : 'Ctrl'}</Kbd>
              <Kbd>K</Kbd> toggle
            </span>
          </div>
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

export const isMac = typeof navigator !== 'undefined' && /Mac|iP(hone|ad|od)/.test(navigator.platform);
