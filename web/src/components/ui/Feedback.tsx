import type { CSSProperties, ReactNode } from 'react';
import { Icon, type IconName } from '../Icon';

export type Tone = 'ok' | 'warn' | 'danger' | 'accent' | 'info' | 'muted';

export function Spinner({ size = 12 }: { size?: number }) {
  return <span className="spinner" style={{ width: size, height: size }} role="status" aria-label="Loading" />;
}

export function Progress({
  value,
  tone,
  live,
  thick,
  label,
}: {
  value: number;
  tone?: 'ok' | 'warn' | 'danger' | 'muted';
  /** Animated sheen while bytes are flowing. */
  live?: boolean;
  thick?: boolean;
  label?: string;
}) {
  const pct = Math.max(0, Math.min(1, value)) * 100;
  const cls = ['progress', tone && `progress-${tone}`, live && 'progress-live', thick && 'progress-thick'].filter(Boolean).join(' ');
  return (
    <div className={cls} role="progressbar" aria-label={label} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(pct)}>
      <div className="progress-bar" style={{ width: `${pct}%` }} />
    </div>
  );
}

export function Badge({
  children,
  tone,
  title,
  outline,
  icon,
}: {
  children: ReactNode;
  tone?: Tone;
  title?: string;
  outline?: boolean;
  icon?: IconName;
}) {
  return (
    <span className={`badge${tone && tone !== 'muted' ? ` badge-${tone}` : ''}${outline ? ' badge-outline' : ''}`} title={title}>
      {icon && <Icon name={icon} size={11} />}
      {children}
    </span>
  );
}

/** Small numeric pill (nav counts, tab counts). Renders nothing for 0 unless `zero`. */
export function Count({ n, accent, zero }: { n: number; accent?: boolean; zero?: boolean }) {
  if (!n && !zero) return null;
  return <span className={`count${accent ? ' count-accent' : ''}`}>{n > 999 ? `${Math.floor(n / 1000)}k` : n}</span>;
}

export function Kbd({ children, className }: { children: ReactNode; className?: string }) {
  return <kbd className={`kbd${className ? ` ${className}` : ''}`}>{children}</kbd>;
}

export function Skeleton({ width, height, style }: { width?: number | string; height?: number; style?: CSSProperties }) {
  return <span className="skeleton" style={{ width, height, ...style }} aria-hidden="true" />;
}

export function Empty({
  children,
  title,
  action,
}: {
  children?: ReactNode;
  title?: ReactNode;
  /** Accepted for compatibility; empty states are text only. */
  icon?: IconName;
  action?: ReactNode;
}) {
  return (
    <div className="empty">
      {title && <div className="empty-title">{title}</div>}
      {children && <p>{children}</p>}
      {action}
    </div>
  );
}

export function ErrorNote({ error }: { error: unknown }) {
  if (!error) return null;
  const msg = error instanceof Error ? error.message : String(error);
  return (
    <p className="error-note" role="alert">
      <Icon name="alert" size={14} style={{ marginTop: 1 }} />
      <span>{msg}</span>
    </p>
  );
}

export function Note({ children, icon = 'info', accent }: { children: ReactNode; icon?: IconName; accent?: boolean }) {
  return (
    <div className={`note${accent ? ' note-accent' : ''}`}>
      <Icon name={icon} size={14} style={{ marginTop: 1, flex: 'none' }} />
      <div>{children}</div>
    </div>
  );
}
