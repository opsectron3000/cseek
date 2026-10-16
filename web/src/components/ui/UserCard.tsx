import { useEffect, useState, type ReactElement } from 'react';
import { PreviewCard } from '@base-ui/react/preview-card';
import { api } from '../../api/client';
import type { UserInfo, UserStats, UserStatusInfo } from '../../api/types';
import { formatSpeed } from '../../lib/format';
import { routes } from '../../lib/router';
import { Link } from '../Link';
import { Avatar, countryName, Flag } from './Identity';

interface Peek {
  status?: UserStatusInfo;
  stats?: UserStats;
  info?: UserInfo;
  /** Info could not be fetched (user offline or unreachable). */
  infoFailed?: boolean;
}

const TTL = 3 * 60_000;
const cache = new Map<string, { at: number; peek: Peek; listeners: Set<(p: Peek) => void> }>();

/** Fetch what the card shows, once per user per few minutes, sharing in-flight requests. */
function usePeek(name: string, active: boolean): Peek {
  const [peek, setPeek] = useState<Peek>(() => cache.get(name)?.peek ?? {});
  useEffect(() => {
    if (!active) return;
    let entry = cache.get(name);
    const stale = !entry || Date.now() - entry.at > TTL;
    if (!entry) {
      entry = { at: 0, peek: {}, listeners: new Set() };
      cache.set(name, entry);
    }
    const e = entry;
    e.listeners.add(setPeek);
    setPeek(e.peek);
    if (stale) {
      e.at = Date.now();
      const patch = (p: Partial<Peek>) => {
        e.peek = { ...e.peek, ...p };
        e.listeners.forEach((l) => l(e.peek));
      };
      api.userStatus(name).then((status) => patch({ status }), () => {});
      api.userStats(name).then((stats) => patch({ stats }), () => {});
      api.userInfo(name).then(
        (info) => patch({ info, infoFailed: false }),
        () => patch({ infoFailed: true }),
      );
    }
    return () => {
      e.listeners.delete(setPeek);
    };
  }, [name, active]);
  return peek;
}

function CardBody({ name, cc }: { name: string; cc?: string | null }) {
  const { status, stats, info, infoFailed } = usePeek(name, true);
  const state = status?.status;
  return (
    <>
      <div className="uc-head">
        <Avatar name={name} size={36} />
        <div className="uc-id">
          <Link to={routes.user(name)} className="uc-name">
            {name}
          </Link>
          <span className="uc-sub">
            {state ? (
              <span className="uc-state">
                <span className={`dot dot-${state}`} aria-hidden="true" />
                {state === 'online' ? 'Online' : state === 'away' ? 'Away' : 'Offline'}
                {status?.isPrivileged && <span className="uc-priv">privileged</span>}
              </span>
            ) : (
              <span className="faint">…</span>
            )}
            {cc && (
              <span className="uc-country">
                <Flag cc={cc} size="sm" tip={false} />
                {countryName(cc)}
              </span>
            )}
          </span>
        </div>
      </div>
      {info?.description?.trim() && <p className="uc-desc">{info.description.trim()}</p>}
      <dl className="uc-facts">
        <div>
          <dt>Files</dt>
          <dd className="num">{stats ? stats.fileCount.toLocaleString() : '–'}</dd>
        </div>
        <div>
          <dt>Speed</dt>
          <dd className="num">{stats ? formatSpeed(stats.averageSpeed) : '–'}</dd>
        </div>
        <div>
          <dt>Slots</dt>
          <dd className="num">
            {info ? (
              <span className={info.hasFreeUploadSlot ? 'uc-free' : undefined}>{info.hasFreeUploadSlot ? 'Free' : `Queue ${info.queueLength.toLocaleString()}`}</span>
            ) : infoFailed ? (
              '–'
            ) : (
              '…'
            )}
          </dd>
        </div>
      </dl>
      <div className="uc-actions">
        <Link to={routes.dm(name)} className="uc-action">
          Message
        </Link>
        <Link to={routes.browse(name)} className="uc-action">
          Browse files
        </Link>
      </div>
    </>
  );
}

/** Wrap a username link so hovering it previews the user. */
export function UserCard({ name, cc, children }: { name: string; cc?: string | null; children: ReactElement }) {
  return (
    <PreviewCard.Root>
      <PreviewCard.Trigger delay={450} closeDelay={150} render={children} />
      <PreviewCard.Portal>
        <PreviewCard.Positioner className="popover-positioner" sideOffset={6} align="start" collisionPadding={12}>
          <PreviewCard.Popup className="popover user-card">
            <CardBody name={name} cc={cc} />
          </PreviewCard.Popup>
        </PreviewCard.Positioner>
      </PreviewCard.Portal>
    </PreviewCard.Root>
  );
}
