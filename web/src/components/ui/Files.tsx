const LOSSLESS = new Set(['flac', 'wav', 'aiff', 'aif', 'alac', 'ape', 'wv', 'dsf', 'dff']);

/** File-type pill; lossless formats get the accent. */
export function Ext({ ext }: { ext: string }) {
  const e = ext.replace(/^\./, '').toLowerCase();
  if (!e || e.length > 5) return null;
  return <span className={`ext${LOSSLESS.has(e) ? ' ext-lossless' : ''}`}>{e}</span>;
}

export const extOf = (filename: string, ext?: string) => ext || (filename.includes('.') ? filename.split('.').pop()! : '');
