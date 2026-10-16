import { memo, useMemo, useState } from 'react';
import { api } from '../api/client';
import type { BrowseResult, Directory, SearchFile } from '../api/types';
import { upsertTransfers } from '../store/app';
import { openBrowse, type BrowseSession } from '../store/browse';
import { formatBytes, formatDuration, formatQuality, plural } from '../lib/format';
import { splitRemote } from '../lib/paths';
import { Icon } from './Icon';
import { toast, toastError } from './Toaster';
import { Button, Empty, ErrorNote, Ext, extOf, IconButton, Input, Skeleton } from './ui';
import './browse.css';

interface TreeNode {
  name: string;
  path: string;
  children: TreeNode[];
  dir: Directory | null;
  locked: boolean;
  /** Total files in this subtree. */
  total: number;
}

function buildTree(result: BrowseResult): TreeNode[] {
  const root: TreeNode = { name: '', path: '', children: [], dir: null, locked: false, total: 0 };
  const index = new Map<string, TreeNode>();
  const insert = (d: Directory, locked: boolean) => {
    const parts = splitRemote(d.name);
    let node = root;
    let path = '';
    for (const part of parts) {
      path = path ? `${path}\\${part}` : part;
      const key = `${locked ? 'L' : ''}${path}`;
      let child = index.get(key);
      if (!child) {
        child = { name: part, path, children: [], dir: null, locked, total: 0 };
        index.set(key, child);
        node.children.push(child);
      }
      node = child;
    }
    node.dir = d;
  };
  result.directories.forEach((d) => insert(d, false));
  result.lockedDirectories.forEach((d) => insert(d, true));
  const finalize = (n: TreeNode): number => {
    n.children.sort((a, b) => a.name.localeCompare(b.name, undefined, { numeric: true }));
    n.total = (n.dir?.files.length ?? 0) + n.children.reduce((s, c) => s + finalize(c), 0);
    return n.total;
  };
  finalize(root);
  return root.children;
}

function collectFiles(n: TreeNode): { dir: Directory; file: SearchFile }[] {
  const out: { dir: Directory; file: SearchFile }[] = [];
  const walk = (x: TreeNode) => {
    if (x.dir) for (const f of x.dir.files) out.push({ dir: x.dir, file: f });
    x.children.forEach(walk);
  };
  walk(n);
  return out;
}

const fullPath = (dir: Directory, f: SearchFile) => (f.filename.includes('\\') ? f.filename : `${dir.name}\\${f.filename}`);

async function queue(username: string, items: { dir: Directory; file: SearchFile }[]) {
  if (!items.length) return;
  try {
    const res = await api.download(items.map(({ dir, file }) => ({ username, filename: fullPath(dir, file), size: file.size })));
    upsertTransfers(res);
    toast(`Queued ${plural(res.length, 'file')}`, { type: 'success' });
  } catch (e) {
    toastError(e, 'Could not queue download');
  }
}

function matchesQuery(n: TreeNode, q: string): boolean {
  if (n.name.toLowerCase().includes(q)) return true;
  if (n.dir?.files.some((f) => f.filename.toLowerCase().includes(q))) return true;
  return n.children.some((c) => matchesQuery(c, q));
}

const TreeItem = memo(function TreeItem({
  node,
  depth,
  selected,
  onSelect,
  q,
}: {
  node: TreeNode;
  depth: number;
  selected: string | null;
  onSelect: (n: TreeNode) => void;
  q: string;
}) {
  const [open, setOpen] = useState(depth < 1);
  const children = q ? node.children.filter((c) => matchesQuery(c, q)) : node.children;
  const expanded = open || (!!q && depth < 6);
  const key = `${node.locked ? 'L' : ''}${node.path}`;
  return (
    <li role="treeitem" aria-expanded={children.length ? expanded : undefined} aria-selected={selected === key}>
      <div className={`tree-row${selected === key ? ' active' : ''}${node.locked ? ' locked' : ''}`} style={{ paddingLeft: depth * 12 + 4 }}>
        {children.length ? (
          <button type="button" className={`tree-toggle${expanded ? ' open' : ''}`} aria-label={expanded ? 'Collapse' : 'Expand'} onClick={() => setOpen(!expanded)}>
            <Icon name="chevronRight" size={11} />
          </button>
        ) : (
          <span className="tree-toggle" />
        )}
        <button
          type="button"
          className="tree-label"
          onClick={() => {
            onSelect(node);
            if (children.length && !expanded) setOpen(true);
          }}
          title={node.path}
        >
          <Icon name={node.locked ? 'lock' : 'folder'} size={13} />
          <span className="truncate">{node.name}</span>
          <span className="tree-count">{node.total}</span>
        </button>
      </div>
      {expanded && children.length > 0 && (
        <ul role="group">
          {children.map((c) => (
            <TreeItem key={c.path} node={c} depth={depth + 1} selected={selected} onSelect={onSelect} q={q} />
          ))}
        </ul>
      )}
    </li>
  );
});

function DirectoryFiles({ username, node }: { username: string; node: TreeNode }) {
  const [sel, setSel] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const files = node.dir?.files ?? [];
  const all = collectFiles(node);
  const allSize = all.reduce((n, x) => n + x.file.size, 0);
  const selected = files.filter((f) => sel.has(f.filename));

  const run = async (items: { dir: Directory; file: SearchFile }[]) => {
    setBusy(true);
    await queue(username, items);
    setBusy(false);
    setSel(new Set());
  };

  return (
    <div className="browse-files">
      <div className="browse-files-head">
        <span className="browse-path truncate" title={node.path}>
          {splitRemote(node.path).map((part, i, arr) => (
            <span key={i} className={i === arr.length - 1 ? 'medium' : 'faint'}>
              {part}
              {i < arr.length - 1 && <span className="crumb-sep">/</span>}
            </span>
          ))}
        </span>
        <span className="spacer" />
        {!node.locked && (
          <>
            {selected.length > 0 && (
              <Button size="sm" variant="primary" icon="download" busy={busy} onClick={() => run(selected.map((file) => ({ dir: node.dir!, file })))}>
                Download {selected.length}
              </Button>
            )}
            <Button size="sm" icon="download" variant={selected.length ? 'default' : 'primary'} busy={busy} disabled={!all.length} onClick={() => run(all)}>
              Folder <span className="btn-meta">{plural(all.length, 'file')} · {formatBytes(allSize)}</span>
            </Button>
          </>
        )}
      </div>
      {files.length === 0 ? (
        <Empty icon="folder">{node.children.length ? `${plural(node.children.length, 'subfolder')}, no files directly in here.` : 'Empty folder.'}</Empty>
      ) : (
        <ul className="files">
          {files.map((f) => (
            <li key={f.filename} className={`file-row${sel.has(f.filename) ? ' selected' : ''}`}>
              <input
                type="checkbox"
                disabled={node.locked}
                aria-label={`Select ${f.filename}`}
                checked={sel.has(f.filename)}
                onChange={(e) =>
                  setSel((prev) => {
                    const n = new Set(prev);
                    if (e.target.checked) n.add(f.filename);
                    else n.delete(f.filename);
                    return n;
                  })
                }
              />
              <span className="file-name" title={f.filename}>
                <Ext ext={extOf(f.filename, f.extension)} />
                <span className="truncate">{splitRemote(f.filename).pop()}</span>
              </span>
              <span className="file-q">{formatQuality(f)}</span>
              <span className="file-len">{f.length ? formatDuration(f.length) : ''}</span>
              <span className="file-size">{formatBytes(f.size)}</span>
              {!node.locked ? (
                <IconButton icon="download" label="Download" className="row-action" onClick={() => queue(username, [{ dir: node.dir!, file: f }])} />
              ) : (
                <span />
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

export function BrowseView({ session }: { session: BrowseSession }) {
  const tree = useMemo(() => (session.result ? buildTree(session.result) : []), [session.result]);
  const [selected, setSelected] = useState<TreeNode | null>(null);
  const [q, setQ] = useState('');
  const ql = q.trim().toLowerCase();

  if (session.loading) {
    return (
      <div className="browse">
        <div className="browse-tree panel">
          <div className="panel-head">
            <span className="small muted">Fetching {session.username}’s share list… this can take up to a minute.</span>
          </div>
          <div className="stack-sm pad">
            {[68, 52, 74, 40, 60, 48].map((w, i) => (
              <Skeleton key={i} width={`${w}%`} />
            ))}
          </div>
        </div>
        <div className="browse-main" />
      </div>
    );
  }
  if (session.error) {
    return (
      <div className="pad stack">
        <ErrorNote error={session.error} />
        <div>
          <Button onClick={() => openBrowse(session.username, true)}>Retry</Button>
        </div>
      </div>
    );
  }
  if (!session.result) return null;
  const totalFiles = session.result.directories.reduce((n, d) => n + d.files.length, 0);
  const visible = ql ? tree.filter((n) => matchesQuery(n, ql)) : tree;

  return (
    <div className="browse">
      <div className="browse-tree panel">
        <div className="panel-head">
          <Input icon="filter" inputSize="sm" placeholder="Filter folders and files" aria-label="Filter shares" value={q} onChange={(e) => setQ(e.target.value)} onClear={() => setQ('')} />
          <IconButton icon="refresh" label="Browse again" onClick={() => openBrowse(session.username, true)} />
        </div>
        <p className="browse-summary">
          {plural(session.result.directories.length, 'folder')} · {plural(totalFiles, 'file')}
          {session.result.lockedDirectories.length ? ` · ${session.result.lockedDirectories.length} locked` : ''}
        </p>
        {visible.length === 0 ? (
          <p className="muted small pad">{tree.length ? 'No matches.' : 'No shared files.'}</p>
        ) : (
          <ul role="tree" aria-label={`${session.username} shares`} className="tree">
            {visible.map((n) => (
              <TreeItem
                key={`${n.locked}${n.path}`}
                node={n}
                depth={0}
                selected={selected ? `${selected.locked ? 'L' : ''}${selected.path}` : null}
                onSelect={setSelected}
                q={ql}
              />
            ))}
          </ul>
        )}
      </div>
      <div className="browse-main">
        {selected ? <DirectoryFiles key={selected.path} username={session.username} node={selected} /> : <Empty icon="folder" title="Pick a folder">Choose a folder on the left to see its files.</Empty>}
      </div>
    </div>
  );
}
