import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { api } from '../api/client';
import type { Options } from '../api/types';
import { camelToLabel } from '../lib/format';
import { invalidateDestinations } from './useDestinations';
import { loadBlacklist } from '../store/app';
import { attempt, toast } from '../components/Toaster';
import { Button, ErrorNote, IconButton, Input, Skeleton, StringList, Switch, Textarea } from '../components/ui';
import { Icon, type IconName } from '../components/Icon';

type Json = string | number | boolean | null | Json[] | { [k: string]: Json };
type Obj = { [k: string]: Json };

const SECRET = '********';
const SECRET_KEYS = /password|apikey|secret|token/i;

/** Templates for new rows in object arrays (path is dot-joined keys with [] for arrays). */
const ROW_TEMPLATES: Record<string, Obj> = {
  'directories.destinations': { name: '', path: '' },
  groups: { name: '', members: [], uploadSlots: 1, speedLimitKbps: 0, priority: 0, allowedExtensions: [], queueLimitFiles: 0 },
};

const SECTION_ICONS: Record<string, IconName> = {
  soulseek: 'globe',
  directories: 'folder',
  shares: 'shares',
  transfers: 'download',
  groups: 'user',
  blacklist: 'ban',
  leechers: 'eye',
  search: 'search',
  rooms: 'hash',
  wishlist: 'wishlist',
  guard: 'lock',
  nowPlaying: 'music',
  integrations: 'link',
  web: 'monitor',
};

const SECTION_HINTS: Record<string, string> = {
  soulseek: 'Network account and connection.',
  directories: 'Where downloads go. Destinations appear as targets when downloading.',
  shares: 'Folders shared with the network.',
  transfers: 'Upload slots, speed limits, multi-source downloads.',
  groups: 'Custom user groups with their own limits.',
  blacklist: 'Users blocked from everything.',
  leechers: 'Users who share less than the thresholds.',
  search: 'Outgoing search limits and filters.',
  rooms: 'Rooms joined automatically.',
  wishlist: 'Background wishlist searches.',
  guard: 'Abuse protection.',
  nowPlaying: 'Publish what you are listening to in your user description.',
  integrations: 'External services.',
  web: 'Web UI and API access. Changes may require a restart.',
};

function blankLike(v: Json): Json {
  if (Array.isArray(v)) return [];
  if (v === null) return null;
  switch (typeof v) {
    case 'string':
      return '';
    case 'number':
      return 0;
    case 'boolean':
      return false;
    default: {
      const o: Obj = {};
      for (const [k, x] of Object.entries(v)) o[k] = blankLike(x);
      return o;
    }
  }
}

const same = (a: Json | undefined, b: Json | undefined) => JSON.stringify(a) === JSON.stringify(b);

/** One settings row: label on the left, control on the right (or below, for `stacked`). */
function Row({ label, sub, changed, stacked, htmlFor, children }: { label: ReactNode; sub?: ReactNode; changed?: boolean; stacked?: boolean; htmlFor?: string; children: ReactNode }) {
  return (
    <div className={`set-row${stacked ? ' set-row-stacked' : ''}${changed ? ' changed' : ''}`}>
      <div className="set-label">
        <label htmlFor={htmlFor}>{label}</label>
        {sub && <span className="set-sub">{sub}</span>}
      </div>
      <div className="set-control">{children}</div>
    </div>
  );
}

function Field({ name, path, value, base, onChange }: { name: string; path: string; value: Json; base: Json | undefined; onChange: (v: Json) => void }) {
  const label = camelToLabel(name);
  const id = `opt-${path}`;
  const changed = base !== undefined && !same(value, base);

  if (typeof value === 'boolean') {
    return (
      <Row label={label} changed={changed} htmlFor={id}>
        <Switch checked={value} onChange={onChange} ariaLabel={label} />
      </Row>
    );
  }
  if (typeof value === 'number') {
    return (
      <Row label={label} changed={changed} htmlFor={id}>
        <Input
          id={id}
          inputSize="sm"
          className="set-num"
          type="number"
          value={Number.isFinite(value) ? value : 0}
          step="any"
          onChange={(e) => onChange(e.target.value === '' ? 0 : Number(e.target.value))}
        />
      </Row>
    );
  }
  if (typeof value === 'string' || value === null) {
    const secret = SECRET_KEYS.test(name);
    const long = name === 'description' || name === 'descriptionTemplate';
    const mono = /path|dir|downloads|incomplete|url|host/i.test(name);
    return (
      <Row label={label} sub={value === null ? 'unset' : undefined} changed={changed} stacked={long} htmlFor={id}>
        {long ? (
          <Textarea id={id} rows={3} value={value ?? ''} onChange={(e) => onChange(e.target.value)} />
        ) : (
          <Input
            id={id}
            inputSize="sm"
            className={`set-text${mono ? ' mono' : ''}`}
            type={secret ? 'password' : 'text'}
            autoComplete="off"
            value={value ?? ''}
            placeholder={secret && value === SECRET ? 'unchanged' : value === null ? 'not set' : undefined}
            onChange={(e) => onChange(e.target.value === '' && value === null ? null : e.target.value)}
          />
        )}
      </Row>
    );
  }
  if (Array.isArray(value)) {
    if (value.every((x) => typeof x === 'string')) {
      return (
        <Row label={label} sub={value.length ? `${value.length}` : undefined} changed={changed} stacked>
          <StringList value={value as string[]} onChange={(v) => onChange(v)} ariaLabel={`Add to ${label}`} />
        </Row>
      );
    }
    const template = ROW_TEMPLATES[path] ?? (value[0] !== undefined ? (blankLike(value[0]) as Obj) : null);
    const baseRows = Array.isArray(base) ? base : [];
    return (
      <div className={`set-group${changed ? ' changed' : ''}`}>
        <div className="set-group-head">
          <span className="set-group-title">{label}</span>
          <span className="set-sub">{value.length}</span>
          <span className="spacer" />
          {template && (
            <Button size="xs" variant="ghost" icon="plus" onClick={() => onChange([...value, structuredClone(template)])}>
              Add
            </Button>
          )}
        </div>
        {value.length === 0 && <p className="set-none">None yet.</p>}
        {value.map((row, i) => (
          <div key={i} className="set-item">
            <div className="set-item-head">
              <span className="set-item-name">
                {typeof row === 'object' && row && !Array.isArray(row) && typeof row.name === 'string' && row.name ? row.name : `#${i + 1}`}
              </span>
              <IconButton icon="trash" label="Remove" variant="danger-ghost" onClick={() => onChange(value.filter((_, j) => j !== i))} />
            </div>
            <ObjectFields
              value={row as Obj}
              base={baseRows[i] as Obj | undefined}
              path={path}
              onChange={(r) => {
                const next = value.slice();
                next[i] = r;
                onChange(next);
              }}
            />
          </div>
        ))}
      </div>
    );
  }
  return (
    <div className="set-group">
      <div className="set-group-head">
        <span className="set-group-title">{label}</span>
      </div>
      <ObjectFields value={value} base={base && typeof base === 'object' && !Array.isArray(base) ? base : undefined} path={path} onChange={onChange} />
    </div>
  );
}

function ObjectFields({ value, base, path, onChange }: { value: Obj; base?: Obj; path: string; onChange: (v: Obj) => void }) {
  return (
    <div className="set-rows">
      {Object.entries(value).map(([k, v]) => (
        <Field key={k} name={k} path={path ? `${path}.${k}` : k} value={v} base={base?.[k]} onChange={(nv) => onChange({ ...value, [k]: nv })} />
      ))}
    </div>
  );
}

function OptionsSkeleton() {
  return (
    <div className="settings">
      <div className="set-nav">
        {[70, 56, 64, 48, 60, 52].map((w, i) => (
          <Skeleton key={i} width={`${w}%`} height={14} style={{ margin: '7px 10px' }} />
        ))}
      </div>
      <div className="set-card">
        {[0, 1, 2, 3, 4].map((i) => (
          <div key={i} className="set-row">
            <Skeleton width={120} height={12} />
            <Skeleton width={200} height={24} />
          </div>
        ))}
      </div>
    </div>
  );
}

export function OptionsForm() {
  const [saved, setSaved] = useState<Obj | null>(null);
  const [draft, setDraft] = useState<Obj | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [busy, setBusy] = useState(false);
  const [section, setSection] = useState<string>('');

  useEffect(() => {
    api.options().then((o) => {
      const obj = o as unknown as Obj;
      setSaved(obj);
      setDraft(structuredClone(obj));
      setSection(Object.keys(obj)[0] ?? '');
    }, setError);
  }, []);

  const dirty = useMemo(() => !!saved && !!draft && !same(saved, draft), [saved, draft]);

  if (error) return <ErrorNote error={error} />;
  if (!draft || !saved) return <OptionsSkeleton />;

  const keys = Object.keys(draft);
  const changedSections = keys.filter((k) => !same(saved[k], draft[k]));
  const save = async () => {
    setBusy(true);
    const res = await attempt(() => api.saveOptions(draft as unknown as Options), 'Could not save options');
    setBusy(false);
    if (res) {
      const obj = res as unknown as Obj;
      setSaved(obj);
      setDraft(structuredClone(obj));
      invalidateDestinations();
      void loadBlacklist();
      toast('Options saved', { type: 'success' });
    }
  };
  const value = section ? draft[section] : undefined;

  return (
    <div className="settings">
      <nav className="set-nav" aria-label="Option sections">
        {keys.map((k) => (
          <button key={k} type="button" className={`set-nav-item${section === k ? ' active' : ''}`} aria-current={section === k ? 'true' : undefined} onClick={() => setSection(k)}>
            {section === k && <span className="set-nav-pill" />}
            <Icon name={SECTION_ICONS[k] ?? 'sliders'} size={14} />
            <span className="truncate">{camelToLabel(k)}</span>
            {changedSections.includes(k) && <span className="dot dot-accent" aria-label="modified" />}
          </button>
        ))}
      </nav>
      <div className="set-main">
        {section && value !== undefined && (
          <section key={section} className="set-card">
            <header className="set-card-head">
              <h2 className="card-title">{camelToLabel(section)}</h2>
              {SECTION_HINTS[section] && <p className="set-sub">{SECTION_HINTS[section]}</p>}
            </header>
            {typeof value === 'object' && value !== null && !Array.isArray(value) ? (
              <ObjectFields value={value} base={saved[section] as Obj} path={section} onChange={(v) => setDraft({ ...draft, [section]: v })} />
            ) : (
              <div className="set-rows">
                <Field name={section} path={section} value={value} base={saved[section]} onChange={(v) => setDraft({ ...draft, [section]: v })} />
              </div>
            )}
          </section>
        )}
          {dirty && (
            <div
              className="set-savebar"
              role="status"
            >
              <span className="dot dot-accent" />
              <span>
                Unsaved changes in <span className="medium">{changedSections.map(camelToLabel).join(', ')}</span>
              </span>
              <span className="spacer" />
              <Button size="sm" variant="ghost" onClick={() => setDraft(structuredClone(saved))}>
                Discard
              </Button>
              <Button size="sm" variant="primary" busy={busy} onClick={save} icon="check">
                Save
              </Button>
            </div>
          )}
      </div>
    </div>
  );
}
