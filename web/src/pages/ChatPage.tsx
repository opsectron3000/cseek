import { useEffect, useMemo, useRef, useState } from 'react';
import { api } from '../api/client';
import type { RoomSummary } from '../api/types';
import { loadConversations, loadRooms, loadThread, setActiveChat, store, useStore } from '../store/app';
import { formatRelative } from '../lib/format';
import { navigate, routes } from '../lib/router';
import { ChatLog, Composer, RoomUserList, type ChatLine } from '../components/Chat';
import { Link } from '../components/Link';
import { attempt } from '../components/Toaster';
import { Icon } from '../components/Icon';
import { ActionMenu, Avatar, Button, Count, ErrorNote, IconButton, Input, User } from '../components/ui';
import './chat.css';

export type ChatView = { kind: 'home' } | { kind: 'rooms' } | { kind: 'user'; name: string } | { kind: 'room'; name: string };

const useMe = () => useStore((s) => s.app?.server.username ?? null);

async function joinAndOpen(name: string) {
  await attempt(async () => {
    const r = await api.joinRoom(name);
    store.set((s) => ({ rooms: { ...s.rooms, [r.name]: r } }));
    navigate(routes.room(r.name));
  });
}

/* ---------------------------------------------------------------- sidebar */

function Sidebar({ view }: { view: ChatView }) {
  const conversations = useStore((s) => s.conversations);
  const rooms = useStore((s) => s.rooms);
  const roomUnread = useStore((s) => s.roomUnread);
  const [q, setQ] = useState('');
  const qt = q.trim();
  const ql = qt.toLowerCase();

  const convs = useMemo(
    () =>
      Object.values(conversations)
        .filter((c) => !ql || c.username.toLowerCase().includes(ql))
        .sort((a, b) => (b.lastMessage?.timestamp ?? '').localeCompare(a.lastMessage?.timestamp ?? '')),
    [conversations, ql],
  );
  const roomList = useMemo(
    () =>
      Object.values(rooms)
        .filter((r) => !ql || r.name.toLowerCase().includes(ql))
        .map((r) => ({ room: r, last: r.messages[r.messages.length - 1] }))
        .sort((a, b) => (b.last?.timestamp ?? '').localeCompare(a.last?.timestamp ?? '') || a.room.name.localeCompare(b.room.name)),
    [rooms, ql],
  );
  const exactUser = convs.some((c) => c.username.toLowerCase() === ql);
  const exactRoom = roomList.some((r) => r.room.name.toLowerCase() === ql);

  const open = () => {
    if (!qt) return;
    const conv = convs.find((c) => c.username.toLowerCase() === ql);
    const room = roomList.find((r) => r.room.name.toLowerCase() === ql);
    setQ('');
    if (room && !conv) navigate(routes.room(room.room.name));
    else navigate(routes.dm(conv?.username ?? qt));
  };

  const isUser = (n: string) => view.kind === 'user' && view.name === n;
  const isRoom = (n: string) => view.kind === 'room' && view.name === n;

  return (
    <aside className="cl" aria-label="Conversations and rooms">
      <div className="cl-head">
        <h1 className="cl-title">Chat</h1>
      </div>
      <div className="cl-find">
        <Input
          icon="search"
          placeholder="Find, or message a user"
          aria-label="Find a conversation or message a user"
          value={q}
          onChange={(e) => setQ(e.target.value)}
          onClear={() => setQ('')}
          onKeyDown={(e) => {
            if (e.key === 'Enter') open();
            if (e.key === 'Escape') setQ('');
          }}
        />
      </div>
      <div className="cl-scroll">
        {qt && (!exactUser || !exactRoom) && (
          <div className="cl-suggest">
            {!exactUser && (
              <button type="button" className="cl-row cl-action" onClick={open}>
                <span className="cl-mark">
                  <Icon name="message" size={15} />
                </span>
                <span className="cl-action-text truncate">
                  Message <b>{qt}</b>
                </span>
                <span className="cl-enter">↵</span>
              </button>
            )}
            {!exactRoom && (
              <button
                type="button"
                className="cl-row cl-action"
                onClick={() => {
                  setQ('');
                  void joinAndOpen(qt);
                }}
              >
                <span className="cl-mark">
                  <Icon name="hash" size={15} />
                </span>
                <span className="cl-action-text truncate">
                  Join or create room <b>{qt}</b>
                </span>
              </button>
            )}
          </div>
        )}

        <div className="cl-sec">Direct</div>
        {convs.length === 0 ? (
          <p className="cl-none">{ql ? 'No matching conversations.' : 'No conversations yet. Type a username above to start one.'}</p>
        ) : (
          convs.map((c) => (
            <Link key={c.username} to={routes.dm(c.username)} className={`cl-row${isUser(c.username) ? ' active' : ''}${c.unread ? ' unread' : ''}`}>
              <span className="cl-av">
                <Avatar name={c.username} size={34} />
                <span className={`dot dot-${c.status}`} role="img" aria-label={c.status} />
              </span>
              <span className="cl-main">
                <span className="cl-line">
                  <span className="cl-name">
                    <User name={c.username} link={false} />
                  </span>
                  {c.lastMessage && <span className="cl-time">{formatRelative(c.lastMessage.timestamp)}</span>}
                </span>
                <span className="cl-line">
                  <span className="cl-preview truncate">
                    {c.lastMessage ? (
                      <>
                        {c.lastMessage.direction === 'out' && <span className="cl-you">You: </span>}
                        {c.lastMessage.message}
                      </>
                    ) : (
                      'No messages'
                    )}
                  </span>
                  <Count n={c.unread} accent />
                </span>
              </span>
            </Link>
          ))
        )}

        <div className="cl-sec">
          <span>Rooms</span>
          <Link to="/chat/rooms" className={`cl-sec-link${view.kind === 'rooms' ? ' active' : ''}`}>
            Browse all
          </Link>
        </div>
        {roomList.length === 0 ? (
          <p className="cl-none">{ql ? 'No matching rooms.' : 'You are not in any rooms.'}</p>
        ) : (
          roomList.map(({ room: r, last }) => {
            const unread = roomUnread[r.name] ?? 0;
            return (
              <Link key={r.name} to={routes.room(r.name)} className={`cl-row${isRoom(r.name) ? ' active' : ''}${unread ? ' unread' : ''}`}>
                <span className="cl-mark">
                  <Icon name={r.isPrivate ? 'lock' : 'hash'} size={15} />
                </span>
                <span className="cl-main">
                  <span className="cl-line">
                    <span className="cl-name truncate">{r.name}</span>
                    {last && <span className="cl-time">{formatRelative(last.timestamp)}</span>}
                  </span>
                  <span className="cl-line">
                    <span className="cl-preview truncate">
                      {last ? (
                        <>
                          <span className="cl-you">{last.self ? 'You' : last.username}: </span>
                          {last.message}
                        </>
                      ) : (
                        `${r.users.length.toLocaleString()} users`
                      )}
                    </span>
                    <Count n={unread} />
                  </span>
                </span>
              </Link>
            );
          })
        )}
      </div>
    </aside>
  );
}

/* ---------------------------------------------------------------- threads */

function BackButton() {
  return <IconButton icon="chevronLeft" label="All chats" className="ch-back" onClick={() => navigate('/chat')} />;
}

function DirectThread({ username }: { username: string }) {
  const thread = useStore((s) => s.messages[username]);
  const conv = useStore((s) => s.conversations[username]);
  const me = useMe();

  useEffect(() => {
    setActiveChat(`u:${username}`);
    void loadThread(username).catch(() => {});
    void api.markRead(username).catch(() => {});
    return () => setActiveChat(null);
  }, [username]);

  const lines: ChatLine[] = useMemo(
    () => (thread ?? []).map((m) => ({ key: m.id, timestamp: m.timestamp, username: m.direction === 'in' ? m.username : 'You', self: m.direction === 'out', message: m.message })),
    [thread],
  );
  const status = conv?.status;

  return (
    <section className="ch" aria-label={`Conversation with ${username}`}>
      <header className="ch-head">
        <BackButton />
        <span className="cl-av">
          <Avatar name={username} size={34} />
          {status && <span className={`dot dot-${status}`} />}
        </span>
        <div className="ch-title">
          <User name={username} className="ch-name" />
          <span className="ch-sub">{status ?? 'Status unknown'}</span>
        </div>
        <div className="ch-actions">
          <IconButton icon="folder" label="Browse files" onClick={() => navigate(routes.browse(username))} />
          <IconButton icon="user" label="Profile" onClick={() => navigate(routes.user(username))} />
          <ActionMenu
            actions={[
              {
                label: 'Delete conversation',
                icon: 'trash',
                danger: true,
                onSelect: () =>
                  void attempt(async () => {
                    await api.deleteConversation(username);
                    store.set((s) => {
                      const { [username]: _c, ...conversations } = s.conversations;
                      const { [username]: _m, ...messages } = s.messages;
                      return { conversations, messages };
                    });
                    navigate('/chat');
                  }),
              },
            ]}
          />
        </div>
      </header>
      <ChatLog lines={lines} me={me} loading={!thread} empty={`No messages with ${username} yet.`} />
      <Composer
        placeholder={`Message ${username}`}
        onSend={async (text) => {
          const m = await api.sendMessage(username, text);
          // The `message` event will also arrive; receiveMessage de-dupes by id.
          store.set((s) => {
            const t = s.messages[username] ?? [];
            if (t.some((x) => x.id === m.id)) return {};
            return {
              messages: { ...s.messages, [username]: [...t, m] },
              conversations: {
                ...s.conversations,
                [username]: { username, unread: 0, status: s.conversations[username]?.status ?? 'offline', lastMessage: m },
              },
            };
          });
        }}
      />
    </section>
  );
}

function RoomThread({ name }: { name: string }) {
  const room = useStore((s) => s.rooms[name]);
  const me = useMe();
  const [showUsers, setShowUsers] = useState(() => window.matchMedia('(min-width: 1100px)').matches);
  const [joining, setJoining] = useState(false);

  useEffect(() => {
    setActiveChat(`r:${name}`);
    return () => setActiveChat(null);
  }, [name]);

  useEffect(() => {
    // Fetch full history once if the event-provided copy has none.
    if (room && room.messages.length === 0) {
      api
        .room(name)
        .then((r) => store.set((s) => ({ rooms: { ...s.rooms, [name]: { ...r, messages: r.messages.length ? r.messages : (s.rooms[name]?.messages ?? []) } } })))
        .catch(() => {});
    }
  }, [name, !!room]);

  const lines: ChatLine[] = useMemo(
    () => (room?.messages ?? []).map((m, i) => ({ key: `${i}${m.timestamp}`, timestamp: m.timestamp, username: m.username, self: m.self, message: m.message })),
    [room?.messages],
  );

  if (!room) {
    return (
      <section className="ch">
        <header className="ch-head">
          <BackButton />
        </header>
        <div className="ch-none">
          <h2>{name}</h2>
          <p>You are not in this room.</p>
          <Button
            variant="primary"
            busy={joining}
            onClick={async () => {
              setJoining(true);
              await joinAndOpen(name);
              setJoining(false);
            }}
          >
            Join room
          </Button>
        </div>
      </section>
    );
  }

  const online = room.users.filter((u) => u.status === 'online').length;
  const ticker = room.tickers.filter((t) => t.message.trim());

  return (
    <section className="ch" aria-label={`Room ${room.name}`}>
      <header className="ch-head">
        <BackButton />
        <span className="cl-mark">
          <Icon name={room.isPrivate ? 'lock' : 'hash'} size={15} />
        </span>
        <div className="ch-title">
          <span className="ch-name truncate">{room.name}</span>
          <span className="ch-sub">
            {room.users.length.toLocaleString()} users · {online.toLocaleString()} online{room.isPrivate ? ' · private' : ''}
          </span>
        </div>
        <div className="ch-actions">
          <IconButton icon="user" label={showUsers ? 'Hide users' : 'Show users'} active={showUsers} onClick={() => setShowUsers((v) => !v)} />
          <ActionMenu
            actions={[
              {
                label: 'Leave room',
                icon: 'logout',
                danger: true,
                onSelect: () =>
                  void attempt(async () => {
                    await api.leaveRoom(name);
                    store.set((s) => {
                      const { [name]: _gone, ...rest } = s.rooms;
                      return { rooms: rest };
                    });
                    navigate('/chat');
                  }),
              },
            ]}
          />
        </div>
      </header>
      {ticker.length > 0 && (
        <div className="ch-ticker" title={ticker.map((t) => `${t.username}: ${t.message}`).join('\n')}>
          {ticker.slice(0, 6).map((t, i) => (
            <span key={i} className="ch-tick">
              <span className="ch-tick-user">{t.username}</span> {t.message}
            </span>
          ))}
        </div>
      )}
      <div className="ch-body">
        <div className="ch-col">
          <ChatLog lines={lines} me={me} empty="No messages since you joined." />
          <Composer placeholder={`Message ${room.name}`} onSend={(t) => api.sendRoomMessage(name, t)} />
        </div>
        {showUsers && <RoomUserList users={room.users} />}
      </div>
    </section>
  );
}

/* ---------------------------------------------------------------- rooms directory */

const PAGE = 200;

function RoomDirectory() {
  const joined = useStore((s) => s.rooms);
  const [available, setAvailable] = useState<RoomSummary[] | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [q, setQ] = useState('');
  const [busy, setBusy] = useState<string | null>(null);
  const [limit, setLimit] = useState(PAGE);
  const inputRef = useRef<HTMLInputElement>(null);

  const refresh = () => {
    setError(null);
    api.availableRooms().then(setAvailable, setError);
  };
  useEffect(() => {
    refresh();
    inputRef.current?.focus();
  }, []);
  useEffect(() => setLimit(PAGE), [q]);

  const qt = q.trim();
  const filtered = useMemo(() => {
    const ql = qt.toLowerCase();
    return (available ?? []).filter((r) => !ql || r.name.toLowerCase().includes(ql)).sort((a, b) => b.userCount - a.userCount);
  }, [available, qt]);
  const exact = available?.some((r) => r.name.toLowerCase() === qt.toLowerCase());

  const join = async (name: string) => {
    setBusy(name);
    await joinAndOpen(name);
    setBusy(null);
  };

  return (
    <section className="ch rd" aria-label="Public rooms">
      <header className="ch-head">
        <BackButton />
        <div className="ch-title">
          <span className="ch-name">Rooms</span>
          <span className="ch-sub">{available ? `${available.length.toLocaleString()} public rooms on the network` : 'Loading the room list…'}</span>
        </div>
        <div className="ch-actions">
          <IconButton icon="refresh" label="Refresh" onClick={refresh} />
        </div>
      </header>
      <div className="rd-find">
        <Input
          ref={inputRef}
          icon="search"
          placeholder="Find or create a room"
          aria-label="Find or create a room"
          value={q}
          onChange={(e) => setQ(e.target.value)}
          onClear={() => setQ('')}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && qt) void join(filtered.find((r) => r.name.toLowerCase() === qt.toLowerCase())?.name ?? filtered[0]?.name ?? qt);
          }}
        />
        {qt && available && !exact && (
          <Button variant="subtle" icon="plus" busy={busy === qt} onClick={() => join(qt)}>
            Create “{qt}”
          </Button>
        )}
      </div>
      <ErrorNote error={error} />
      <div className="rd-scroll">
        {available && filtered.length === 0 ? (
          <p className="ch-quiet">No rooms match “{qt}”.</p>
        ) : (
          <div className="rd-table" role="table" aria-label="Rooms">
            <div className="rd-row rd-headrow" role="row">
              <span role="columnheader">Room</span>
              <span role="columnheader" className="num">
                Users
              </span>
              <span role="columnheader" />
            </div>
            {filtered.slice(0, limit).map((r) => {
              const isJoined = !!joined[r.name];
              return (
                <div key={r.name} role="row" className={`rd-row${isJoined ? ' joined' : ''}`} onClick={() => (isJoined ? navigate(routes.room(r.name)) : join(r.name))}>
                  <span role="cell" className="rd-name">
                    <Icon name={r.isPrivate ? 'lock' : 'hash'} size={14} />
                    <span className="truncate">{r.name}</span>
                  </span>
                  <span role="cell" className="num rd-n">
                    {r.userCount.toLocaleString()}
                  </span>
                  <span role="cell" className="rd-act">
                    <Button
                      size="sm"
                      variant={isJoined ? 'ghost' : 'subtle'}
                      busy={busy === r.name}
                      onClick={(e) => {
                        e.stopPropagation();
                        if (isJoined) navigate(routes.room(r.name));
                        else void join(r.name);
                      }}
                    >
                      {isJoined ? 'Open' : 'Join'}
                    </Button>
                  </span>
                </div>
              );
            })}
          </div>
        )}
        {filtered.length > limit && (
          <div className="rd-more">
            <Button variant="ghost" onClick={() => setLimit((n) => n + PAGE)}>
              Show more ({(filtered.length - limit).toLocaleString()} left)
            </Button>
          </div>
        )}
      </div>
    </section>
  );
}

/* ---------------------------------------------------------------- page */

function Nothing() {
  return (
    <section className="ch">
      <div className="ch-none">
        <h2>No chat open</h2>
        <p>Pick a conversation, type a username on the left to message someone, or browse the public rooms.</p>
        <Button icon="hash" onClick={() => navigate('/chat/rooms')}>
          Browse rooms
        </Button>
      </div>
    </section>
  );
}

export function ChatPage({ view }: { view: ChatView }) {
  useEffect(() => {
    void loadConversations().catch(() => {});
    void loadRooms().catch(() => {});
  }, []);

  return (
    <div className={`chat-page${view.kind !== 'home' ? ' has-main' : ''}`}>
      <Sidebar view={view} />
      <div className="chat-main">
        {view.kind === 'user' ? (
          <DirectThread key={view.name} username={view.name} />
        ) : view.kind === 'room' ? (
          <RoomThread key={view.name} name={view.name} />
        ) : view.kind === 'rooms' ? (
          <RoomDirectory />
        ) : (
          <Nothing />
        )}
      </div>
    </div>
  );
}
