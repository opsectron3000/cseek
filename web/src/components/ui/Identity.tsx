import { useStore } from '../../store/app';
import { routes } from '../../lib/router';
import { Link } from '../Link';
import { Tip } from './Button';
import { UserCard } from './UserCard';

let regionNames: Intl.DisplayNames | null = null;
export function countryName(cc: string): string {
  try {
    regionNames ??= new Intl.DisplayNames(undefined, { type: 'region' });
    return regionNames.of(cc.toUpperCase()) ?? cc;
  } catch {
    return cc;
  }
}

/** Country flag (flag-icons SVG), with the country name as tooltip. */
export function Flag({ cc, size = 'md', tip = true }: { cc: string | null | undefined; size?: 'sm' | 'md'; tip?: boolean }) {
  if (!cc || !/^[A-Za-z]{2}$/.test(cc)) return null;
  const name = countryName(cc);
  const el = <span className={`fi fi-${cc.toLowerCase()} flag${size === 'sm' ? ' flag-sm' : ''}`} role="img" aria-label={name} />;
  return tip ? <Tip label={name}>{el}</Tip> : el;
}

/** Country code we know for a user (from the server or GeoIP), if any. */
export function useCountry(username: string): string | undefined {
  return useStore((s) => s.countries[username]);
}

/**
 * Username with its flag. Links to the user page unless `link={false}`;
 * hovering the link previews the user.
 * `country` overrides the store lookup.
 */
export function User({
  name,
  country,
  link = true,
  status,
  className,
}: {
  name: string;
  country?: string | null;
  link?: boolean;
  status?: 'online' | 'away' | 'offline' | string;
  className?: string;
}) {
  const known = useCountry(name);
  const cc = country ?? known;
  return (
    <span className={`user${className ? ` ${className}` : ''}`}>
      {status && <span className={`dot dot-${status}`} role="img" aria-label={status} />}
      {cc && <Flag cc={cc} size="sm" tip={false} />}
      {link ? (
        <UserCard name={name} cc={cc}>
          <Link to={routes.user(name)} className="user-link user-name">
            {name}
          </Link>
        </UserCard>
      ) : (
        <span className="user-name medium" title={cc ? `${name} · ${countryName(cc)}` : name}>
          {name}
        </span>
      )}
    </span>
  );
}

function hue(s: string): number {
  let h = 0;
  for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) | 0;
  return Math.abs(h) % 360;
}

/** Initial-letter avatar tinted by a stable hash of the name. */
export function Avatar({ name, size = 26 }: { name: string; size?: number }) {
  const initial = (name.match(/[\p{L}\p{N}]/u)?.[0] ?? '?').toUpperCase();
  return (
    <span
      className="avatar-initial"
      aria-hidden="true"
      style={{ width: size, height: size, fontSize: Math.round(size * 0.42), ['--av' as string]: `oklch(0.68 0.13 ${hue(name)})` }}
    >
      {initial}
    </span>
  );
}

/** Plain username text (no link, no flag); truncated unless `full`. */
export function UserName({ name, full }: { name: string; full?: boolean }) {
  return (
    <span className={`user-name${full ? '' : ' truncate'}`} title={name}>
      {name}
    </span>
  );
}
