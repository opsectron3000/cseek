import { useEffect, useState, type FormEvent, type ReactNode } from 'react';
import { api } from '../api/client';
import type { Interests, ItemInfo, Recommendations, ScoredItem, SimilarUser } from '../api/types';
import { startSearch } from '../lib/actions';
import { attempt, toastError } from '../components/Toaster';
import { Button, Count, Empty, ErrorNote, IconButton, Input, PageHeader, Skeleton, TabPanel, Tabs, User } from '../components/ui';
import { Icon } from '../components/Icon';
import './discovery.css';

function useAsync<T>(fn: () => Promise<T>) {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(true);
  const reload = () => {
    setLoading(true);
    setError(null);
    fn()
      .then(setData, setError)
      .finally(() => setLoading(false));
  };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(reload, []);
  return { data, error, loading, reload, setData };
}

function ListSkeleton({ n = 6 }: { n?: number }) {
  return (
    <div className="scored-list" aria-busy="true">
      {Array.from({ length: n }, (_, i) => (
        <div key={i} className="scored-item">
          <Skeleton width={`${35 + ((i * 23) % 45)}%`} height={12} />
        </div>
      ))}
    </div>
  );
}

function InterestColumn({ kind, items, onChange }: { kind: 'like' | 'hate'; items: string[]; onChange: (i: Interests) => void }) {
  const [draft, setDraft] = useState('');
  const add = async (e: FormEvent) => {
    e.preventDefault();
    const v = draft.trim();
    if (!v) return;
    const res = await attempt(() => api.addInterest(v, kind));
    if (res) {
      onChange(res);
      setDraft('');
    }
  };
  const like = kind === 'like';
  return (
    <div className="interest-col">
      <div className="interest-head">
        <Icon name={like ? 'heart' : 'ban'} size={13} />
        <span className="interest-title">{like ? 'Likes' : 'Hates'}</span>
        <Count n={items.length} />
      </div>
      <form onSubmit={add}>
        <Input
          inputSize="sm"
          icon="plus"
          placeholder={like ? 'Add a genre, artist, anything…' : 'Add something to steer away from…'}
          aria-label={`Add ${kind}`}
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          trailing={draft.trim() ? <span className="small faint">↵</span> : undefined}
        />
      </form>
      {items.length === 0 ? (
        <p className="small faint">{like ? 'Nothing yet. Likes drive your recommendations.' : 'Nothing yet.'}</p>
      ) : (
        <ul className="chips">
            {items.map((it) => (
              <li
                key={it}
                className={`chip${like ? ' chip-on' : ''}`}
              >
                <span className="chip-text">{it}</span>
                <button
                  type="button"
                  className="chip-x"
                  aria-label={`Remove ${it}`}
                  onClick={async () => {
                    const res = await attempt(() => api.removeInterest(it, kind));
                    if (res) onChange(res);
                  }}
                >
                  <Icon name="x" size={10} />
                </button>
              </li>
            ))}
        </ul>
      )}
    </div>
  );
}

function Section({ title, count, children }: { title: string; count?: number; children: ReactNode }) {
  return (
    <section className="disc-section">
      <div className="disc-section-head">
        <span>{title}</span>
        {count !== undefined && <span className="faint num">{count}</span>}
      </div>
      {children}
    </section>
  );
}

function ScoredList({ items, onPick, onLike, empty = 'Nothing here yet.' }: { items: ScoredItem[]; onPick: (item: string) => void; onLike: (item: string) => void; empty?: string }) {
  if (!items.length) return <p className="small faint disc-none">{empty}</p>;
  const max = Math.max(1, ...items.map((r) => Math.abs(r.score)));
  return (
    <ul className="scored-list">
      {items.map((r) => (
        <li key={r.item} className="scored-item" style={{ ['--score' as string]: `${(Math.abs(r.score) / max) * 100}%` }}>
          <button type="button" className="scored-name" onClick={() => onPick(r.item)} title={`Look up “${r.item}”`}>
            <span className="truncate">{r.item}</span>
          </button>
          <span className="scored-val">{r.score}</span>
          <span className="scored-actions">
            <IconButton icon="search" label={`Search the network for ${r.item}`} onClick={() => startSearch(r.item).catch((e) => toastError(e, 'Search failed'))} />
            <IconButton icon="heart" label={`Like ${r.item}`} onClick={() => onLike(r.item)} />
          </span>
        </li>
      ))}
    </ul>
  );
}

function Refresh({ loading, onClick }: { loading: boolean; onClick: () => void }) {
  return (
    <div className="disc-bar">
      <IconButton icon="refresh" label="Refresh" onClick={onClick} disabled={loading} />
    </div>
  );
}

function RecPanel({ load, onPick, onLike }: { load: () => Promise<Recommendations>; onPick: (s: string) => void; onLike: (s: string) => void }) {
  const r = useAsync(load);
  return (
    <div className="stack-sm">
      <Refresh loading={r.loading} onClick={r.reload} />
      <ErrorNote error={r.error} />
      <div className="disc-grid">
        <Section title="Recommended" count={r.data?.recommendations.length}>
          {r.data ? <ScoredList items={r.data.recommendations} onPick={onPick} onLike={onLike} empty="No recommendations yet. Add a few likes." /> : r.loading && <ListSkeleton />}
        </Section>
        <Section title="Unrecommended" count={r.data?.unrecommendations.length}>
          {r.data ? <ScoredList items={r.data.unrecommendations} onPick={onPick} onLike={onLike} /> : r.loading && <ListSkeleton />}
        </Section>
      </div>
    </div>
  );
}

function SimilarUsers() {
  const r = useAsync<SimilarUser[]>(api.similarUsers);
  const users = r.data ? [...r.data].sort((a, b) => b.rating - a.rating) : [];
  const max = Math.max(1, ...users.map((u) => u.rating));
  return (
    <div className="stack-sm">
      <Refresh loading={r.loading} onClick={r.reload} />
      <ErrorNote error={r.error} />
      {!r.data ? (
        r.loading && <ListSkeleton n={8} />
      ) : users.length === 0 ? (
        <Empty icon="user" title="No similar users yet">
          The server matches you with people who share your likes. Add some first.
        </Empty>
      ) : (
        <ul className="scored-list disc-section">
          {users.map((u) => (
            <li key={u.username} className="scored-item" style={{ ['--score' as string]: `${(u.rating / max) * 100}%` }}>
              <User name={u.username} className="scored-name" />
              <span className="scored-val">{u.rating}</span>
              <span className="scored-actions" />
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function ItemLookup({ item, setItem, onLike }: { item: string; setItem: (s: string) => void; onLike: (s: string) => void }) {
  const [draft, setDraft] = useState(item);
  const [info, setInfo] = useState<ItemInfo | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(false);
  useEffect(() => setDraft(item), [item]);
  useEffect(() => {
    if (!item) return;
    setLoading(true);
    setError(null);
    api
      .itemInfo(item)
      .then(setInfo, setError)
      .finally(() => setLoading(false));
  }, [item]);
  return (
    <div className="stack">
      <form
        className="row gap-xs wrap"
        onSubmit={(e) => {
          e.preventDefault();
          if (draft.trim()) setItem(draft.trim());
        }}
      >
        <div className="disc-lookup">
          <Input icon="search" placeholder="Any item, e.g. shoegaze" aria-label="Item to look up" value={draft} onChange={(e) => setDraft(e.target.value)} onClear={() => setDraft('')} />
        </div>
        <Button type="submit" variant="primary" disabled={!draft.trim()} busy={loading}>
          Look up
        </Button>
        {item && (
          <Button variant="ghost" icon="heart" onClick={() => onLike(item)}>
            Like “{item}”
          </Button>
        )}
      </form>
      <ErrorNote error={error} />
      {!item ? (
        <Empty icon="discovery" title="Explore an item">
          See related items and who else likes it. Click any recommendation to jump here.
        </Empty>
      ) : (
        <div className="disc-grid">
          <Section title="Related items" count={info && !loading ? info.recommendations.length : undefined}>
            {info && !loading ? <ScoredList items={info.recommendations} onPick={setItem} onLike={onLike} /> : <ListSkeleton />}
          </Section>
          <Section title="Users who like it" count={info && !loading ? info.similarUsers.length : undefined}>
            {info && !loading ? (
              info.similarUsers.length ? (
                <ul className="scored-list">
                  {info.similarUsers.map((u) => (
                    <li key={u} className="scored-item">
                      <User name={u} className="scored-name" />
                    </li>
                  ))}
                </ul>
              ) : (
                <p className="small faint disc-none">Nobody yet.</p>
              )
            ) : (
              <ListSkeleton />
            )}
          </Section>
        </div>
      )}
    </div>
  );
}

type DiscTab = 'recs' | 'global' | 'similar' | 'item';

export function DiscoveryPage() {
  const interests = useAsync<Interests>(api.interests);
  const [tab, setTab] = useState<DiscTab>('recs');
  const [item, setItem] = useState('');
  const like = async (it: string) => {
    const res = await attempt(() => api.addInterest(it, 'like'));
    if (res) interests.setData(res);
  };
  const pick = (it: string) => {
    setItem(it);
    setTab('item');
  };

  return (
    <div className="page">
      <PageHeader title="Discovery" sub="Recommendations from the network, based on what you like" />
      <section className="interests">
        {interests.loading && !interests.data ? (
          <div className="interests-grid">
            {[0, 1].map((i) => (
              <div key={i} className="interest-col">
                <Skeleton width={60} height={12} />
                <Skeleton width="100%" height={26} />
                <Skeleton width="70%" height={20} />
              </div>
            ))}
          </div>
        ) : (
          <>
            <ErrorNote error={interests.error} />
            {interests.data && (
              <div className="interests-grid">
                <InterestColumn kind="like" items={interests.data.likes} onChange={interests.setData} />
                <InterestColumn kind="hate" items={interests.data.hates} onChange={interests.setData} />
              </div>
            )}
          </>
        )}
      </section>
      <Tabs<DiscTab>
        value={tab}
        onChange={setTab}
        ariaLabel="Discovery views"
        tabs={[
          { value: 'recs', label: 'For you', icon: 'star' },
          { value: 'global', label: 'Global', icon: 'globe' },
          { value: 'similar', label: 'Similar users', icon: 'user' },
          { value: 'item', label: 'Item lookup', icon: 'search' },
        ]}
      >
        <TabPanel value="recs">
          <RecPanel load={api.recommendations} onPick={pick} onLike={like} />
        </TabPanel>
        <TabPanel value="global">
          <RecPanel load={api.globalRecommendations} onPick={pick} onLike={like} />
        </TabPanel>
        <TabPanel value="similar">
          <SimilarUsers />
        </TabPanel>
        <TabPanel value="item">
          <ItemLookup item={item} setItem={setItem} onLike={like} />
        </TabPanel>
      </Tabs>
    </div>
  );
}
