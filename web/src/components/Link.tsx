import type { AnchorHTMLAttributes, MouseEvent } from 'react';
import { navigate } from '../lib/router';
import { User } from './ui/Identity';

export function Link({ to, onClick, ...rest }: AnchorHTMLAttributes<HTMLAnchorElement> & { to: string }) {
  return (
    <a
      href={to}
      onClick={(e: MouseEvent<HTMLAnchorElement>) => {
        onClick?.(e);
        if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
        e.preventDefault();
        navigate(to);
      }}
      {...rest}
    />
  );
}

/** Clickable username with its country flag: opens the Users page. */
export function UserLink({ name, className }: { name: string; className?: string }) {
  return <User name={name} className={className} />;
}

export function StatusDot({ status }: { status: 'online' | 'away' | 'offline' | string }) {
  return <span className={`dot dot-${status}`} role="img" aria-label={status} title={status} />;
}
