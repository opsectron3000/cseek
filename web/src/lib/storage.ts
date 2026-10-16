export function load<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(`cseek.${key}`);
    return raw === null ? fallback : (JSON.parse(raw) as T);
  } catch {
    return fallback;
  }
}

export function save(key: string, value: unknown): void {
  try {
    localStorage.setItem(`cseek.${key}`, JSON.stringify(value));
  } catch {
    /* ignore */
  }
}
