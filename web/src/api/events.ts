import { eventsUrl } from './client';
import type { ServerEvent } from './types';

export interface EventStreamHandlers {
  onEvent: (event: ServerEvent) => void;
  onStatus: (connected: boolean) => void;
  /** Called after every successful (re)connect, so callers can resync REST state. */
  onOpen?: (isReconnect: boolean) => void;
}

/**
 * WebSocket event stream with exponential backoff reconnect (1s .. 30s, jittered).
 * Returns a function that closes the stream for good.
 */
export function connectEvents(handlers: EventStreamHandlers): () => void {
  let ws: WebSocket | null = null;
  let closed = false;
  let attempt = 0;
  let everOpened = false;
  let timer: ReturnType<typeof setTimeout> | null = null;

  const open = () => {
    if (closed) return;
    ws = new WebSocket(eventsUrl());
    ws.onopen = () => {
      attempt = 0;
      handlers.onStatus(true);
      handlers.onOpen?.(everOpened);
      everOpened = true;
    };
    ws.onmessage = (msg) => {
      if (typeof msg.data !== 'string') return;
      try {
        const parsed = JSON.parse(msg.data) as ServerEvent;
        if (parsed && typeof parsed.type === 'string') handlers.onEvent(parsed);
      } catch {
        /* ignore malformed frame */
      }
    };
    ws.onclose = () => {
      ws = null;
      handlers.onStatus(false);
      if (closed) return;
      const base = Math.min(30_000, 1000 * 2 ** attempt);
      attempt++;
      timer = setTimeout(open, base / 2 + Math.random() * (base / 2));
    };
    ws.onerror = () => {
      ws?.close();
    };
  };

  open();

  return () => {
    closed = true;
    if (timer) clearTimeout(timer);
    ws?.close();
  };
}
