import { Fragment, useLayoutEffect, useMemo, useRef, useState, type FormEvent, type ReactNode } from 'react';
import type { RoomUser } from '../api/types';
import { formatSpeed, plural } from '../lib/format';
import { Avatar, countryName, IconButton, Input, User } from './ui';
import { toastError } from './Toaster';

export interface ChatLine {
  key: string;
  timestamp: string;
  username: string;
  self: boolean;
  message: string;
}

const GROUP_GAP = 5 * 60_000;

const clock = (iso: string) => new Date(iso).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });

function dayLabel(iso: string): string {
  const d = new Date(iso);
  const today = new Date();
  const yesterday = new Date(today.getTime() - 86_400_000);
  if (d.toDateString() === today.toDateString()) return 'Today';
  if (d.toDateString() === yesterday.toDateString()) return 'Yesterday';
  return d.toLocaleDateString(undefined, { weekday: 'short', month: 'short', day: 'numeric', year: d.getFullYear() === today.getFullYear() ? undefined : 'numeric' });
}

const escapeRe = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
const URL_RE = /\bhttps?:\/\/[^\s<>"']+[^\s<>"'.,;:!?)\]]/;

/** Message text with links made clickable and mentions of `me` marked. */
export function RichText({ text, me }: { text: string; me?: string | null }) {
  const parts = useMemo(() => {
    const sources = [URL_RE.source];
    if (me) sources.push(`(?<![\\w])${escapeRe(me)}(?![\\w])`);
    const re = new RegExp(sources.map((s) => `(${s})`).join('|'), 'gi');
    const out: ReactNode[] = [];
    let last = 0;
    for (const m of text.matchAll(re)) {
      const i = m.index ?? 0;
      if (i > last) out.push(text.slice(last, i));
      if (m[1]) {
        out.push(
          <a key={i} href={m[1]} target="_blank" rel="noreferrer noopener" className="msg-link">
            {m[1]}
          </a>,
        );
      } else {
        out.push(
          <mark key={i} className="msg-mention">
            {m[0]}
          </mark>,
        );
      }
      last = i + m[0].length;
    }
    if (last < text.length) out.push(text.slice(last));
    return out;
  }, [text, me]);
  return <>{parts}</>;
}

export function mentions(text: string, me: string | null | undefined): boolean {
  return !!me && new RegExp(`(?<![\\w])${escapeRe(me)}(?![\\w])`, 'i').test(text);
}

/** Scrolling message list that sticks to the bottom unless the user scrolled up. */
export function ChatLog({
  lines,
  me,
  empty,
  loading,
}: {
  lines: ChatLine[];
  /** Own username: mentions of it are highlighted. */
  me?: string | null;
  empty?: ReactNode;
  loading?: boolean;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const stick = useRef(true);

  useLayoutEffect(() => {
    const el = ref.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [lines]);

  return (
    <div
      ref={ref}
      className="chat-log"
      role="log"
      aria-live="polite"
      onScroll={(e) => {
        const el = e.currentTarget;
        stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
      }}
    >
      {lines.length === 0 ? (
        <div className="chat-log-empty">{loading ? 'Loading…' : (empty ?? 'No messages yet.')}</div>
      ) : (
        <div className="chat-log-inner">
          {lines.map((l, i) => {
            const prev = lines[i - 1];
            const t = Date.parse(l.timestamp);
            const newDay = !prev || new Date(prev.timestamp).toDateString() !== new Date(l.timestamp).toDateString();
            const continued = !newDay && prev && prev.username === l.username && prev.self === l.self && t - Date.parse(prev.timestamp) < GROUP_GAP;
            const text = l.message.replace(/\s+$/, '');
            const action = text.startsWith('/me ');
            const mentioned = !l.self && mentions(l.message, me);
            return (
              <Fragment key={l.key}>
                {newDay && (
                  <div className="chat-day" role="separator">
                    {dayLabel(l.timestamp)}
                  </div>
                )}
                <div className={`msg${l.self ? ' self' : ''}${continued ? ' cont' : ''}${mentioned ? ' mentioned' : ''}`}>
                  <div className="msg-gutter">
                    {continued ? (
                      <time className="msg-hover-time" dateTime={l.timestamp} title={new Date(l.timestamp).toLocaleString()}>
                        {clock(l.timestamp)}
                      </time>
                    ) : (
                      <Avatar name={l.username} size={30} />
                    )}
                  </div>
                  <div className="msg-body">
                    {!continued && (
                      <div className="msg-meta">
                        {l.self ? <span className="msg-you">You</span> : <User name={l.username} />}
                        <time dateTime={l.timestamp} title={new Date(l.timestamp).toLocaleString()}>
                          {clock(l.timestamp)}
                        </time>
                      </div>
                    )}
                    <div className={`msg-text${action ? ' msg-action' : ''}`}>
                      {action ? (
                        <>
                          {l.self ? 'You' : l.username} <RichText text={text.slice(4)} me={me} />
                        </>
                      ) : (
                        <RichText text={text} me={me} />
                      )}
                    </div>
                  </div>
                </div>
              </Fragment>
            );
          })}
        </div>
      )}
    </div>
  );
}

export function Composer({ onSend, placeholder, disabled }: { onSend: (text: string) => Promise<unknown>; placeholder: string; disabled?: boolean }) {
  const [text, setText] = useState('');
  const [busy, setBusy] = useState(false);
  const ref = useRef<HTMLTextAreaElement>(null);

  // Grow with content up to the CSS max-height.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = 'auto';
    el.style.height = `${el.scrollHeight}px`;
  }, [text]);

  const submit = async (e?: FormEvent) => {
    e?.preventDefault();
    const v = text.trim();
    if (!v || busy) return;
    setBusy(true);
    try {
      await onSend(v);
      setText('');
    } catch (err) {
      toastError(err, 'Message not sent');
    } finally {
      setBusy(false);
      ref.current?.focus();
    }
  };

  return (
    <form className="composer" onSubmit={submit}>
      <div className={`composer-box${disabled ? ' disabled' : ''}`} onClick={() => ref.current?.focus()}>
        <textarea
          ref={ref}
          className="composer-input"
          rows={1}
          value={text}
          placeholder={placeholder}
          aria-label={placeholder}
          disabled={disabled}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
              e.preventDefault();
              void submit();
            }
          }}
        />
        <IconButton
          type="submit"
          icon="send"
          label="Send"
          kbd="↵"
          variant={text.trim() ? 'primary' : 'ghost'}
          className="composer-send"
          disabled={disabled || busy || !text.trim()}
        />
      </div>
    </form>
  );
}

export function RoomUserList({ users }: { users: RoomUser[] }) {
  const [q, setQ] = useState('');
  const sorted = useMemo(() => {
    const ql = q.toLowerCase();
    return users
      .filter((u) => !ql || u.username.toLowerCase().includes(ql))
      .sort((a, b) => (a.status === b.status ? a.username.localeCompare(b.username) : a.status === 'online' ? -1 : b.status === 'online' ? 1 : 0));
  }, [users, q]);
  return (
    <aside className="room-users" aria-label="Room users">
      <div className="room-users-head">
        <Input icon="filter" inputSize="sm" placeholder={`Filter ${plural(users.length, 'user')}`} aria-label="Filter users" value={q} onChange={(e) => setQ(e.target.value)} onClear={() => setQ('')} />
      </div>
      <ul className="room-user-list">
        {sorted.slice(0, 1000).map((u) => (
          <li
            key={u.username}
            className={`room-user${u.status !== 'online' ? ' idle' : ''}`}
            title={`${formatSpeed(u.averageSpeed)} · ${u.fileCount.toLocaleString()} files${u.country ? ` · ${countryName(u.country)}` : ''}`}
          >
            <span className={`dot dot-${u.status}`} role="img" aria-label={u.status} />
            <User name={u.username} country={u.country} />
          </li>
        ))}
        {sorted.length === 0 && <li className="room-user faint">No matches</li>}
      </ul>
    </aside>
  );
}
