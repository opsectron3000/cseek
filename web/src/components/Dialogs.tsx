import { useState, type ReactNode } from 'react';
import { Dialog } from '@base-ui/react/dialog';
import { AlertDialog } from '@base-ui/react/alert-dialog';
import { Button } from './ui';
import { Icon } from './Icon';
import './dialogs.css';

export function Modal({
  open,
  onOpenChange,
  title,
  description,
  children,
  footer,
  wide,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: ReactNode;
  description?: ReactNode;
  children: ReactNode;
  footer?: ReactNode;
  wide?: boolean;
}) {
  return (
    <Dialog.Root open={open} onOpenChange={(o) => onOpenChange(o)}>
      <Dialog.Portal>
        <Dialog.Backdrop className="dialog-backdrop" />
        <Dialog.Popup className={`dialog${wide ? ' dialog-wide' : ''}`}>
          <div className="dialog-head">
            <Dialog.Title className="dialog-title">{title}</Dialog.Title>
            <Dialog.Close className="btn btn-ghost btn-icon btn-sm" aria-label="Close">
              <Icon name="x" size={14} />
            </Dialog.Close>
          </div>
          {description && <Dialog.Description className="dialog-desc">{description}</Dialog.Description>}
          <div className="dialog-body">{children}</div>
          {footer && <div className="dialog-foot">{footer}</div>}
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

/** Controlled confirm dialog (Base UI AlertDialog). */
export function ConfirmDialog({
  open,
  onOpenChange,
  title,
  description,
  confirmLabel = 'Confirm',
  danger,
  onConfirm,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: ReactNode;
  description?: ReactNode;
  confirmLabel?: string;
  danger?: boolean;
  onConfirm: () => void | Promise<void>;
}) {
  const [busy, setBusy] = useState(false);
  return (
    <AlertDialog.Root open={open} onOpenChange={(o) => onOpenChange(o)}>
      <AlertDialog.Portal>
        <AlertDialog.Backdrop className="dialog-backdrop" />
        <AlertDialog.Popup className="dialog dialog-narrow">
          <div className="dialog-head dialog-head-confirm">
            {danger && (
              <span className="dialog-icon dialog-icon-danger" aria-hidden="true">
                <Icon name="alert" size={15} />
              </span>
            )}
            <AlertDialog.Title className="dialog-title">{title}</AlertDialog.Title>
          </div>
          {description && <AlertDialog.Description className="dialog-desc">{description}</AlertDialog.Description>}
          <div className="dialog-foot">
            <AlertDialog.Close className="btn btn-default btn-md">Cancel</AlertDialog.Close>
            <Button
              autoFocus
              variant={danger ? 'danger' : 'primary'}
              busy={busy}
              onClick={async () => {
                setBusy(true);
                try {
                  await onConfirm();
                  onOpenChange(false);
                } finally {
                  setBusy(false);
                }
              }}
            >
              {confirmLabel}
            </Button>
          </div>
        </AlertDialog.Popup>
      </AlertDialog.Portal>
    </AlertDialog.Root>
  );
}
