import { Toast } from '@base-ui/react/toast';
import type { ReactNode } from 'react';
import { Icon } from './Icon';

export const toastManager = Toast.createToastManager();

export function toast(title: string, opts: { description?: string; type?: 'error' | 'success' | 'info' } = {}): void {
  toastManager.add({ title, description: opts.description, type: opts.type ?? 'info', timeout: opts.type === 'error' ? 7000 : 3500 });
}

export function toastError(e: unknown, title = 'Request failed'): void {
  const description = e instanceof Error ? e.message : String(e);
  toast(title, { description, type: 'error' });
}

/** Run an async action, toast on failure. Returns the result or undefined. */
export async function attempt<T>(fn: () => Promise<T>, errorTitle?: string): Promise<T | undefined> {
  try {
    return await fn();
  } catch (e) {
    toastError(e, errorTitle);
    return undefined;
  }
}

function ToastList() {
  const { toasts } = Toast.useToastManager();
  return toasts.map((t) => (
    <Toast.Root key={t.id} toast={t} className={`toast toast-${t.type ?? 'info'}`}>
      <Toast.Content className="toast-content">
        <Toast.Title className="toast-title" />
        <Toast.Description className="toast-desc" />
      </Toast.Content>
      <Toast.Close className="btn btn-ghost btn-icon toast-close" aria-label="Dismiss">
        <Icon name="x" size={12} />
      </Toast.Close>
    </Toast.Root>
  ));
}

export function ToastProvider({ children }: { children: ReactNode }) {
  return (
    <Toast.Provider toastManager={toastManager} limit={4}>
      {children}
      <Toast.Portal>
        <Toast.Viewport className="toast-viewport">
          <ToastList />
        </Toast.Viewport>
      </Toast.Portal>
    </Toast.Provider>
  );
}
