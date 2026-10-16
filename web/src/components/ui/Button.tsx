import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from 'react';
import { Tooltip } from '@base-ui/react/tooltip';
import { Icon, type IconName } from '../Icon';
import { Spinner } from './Feedback';
import { Kbd } from './Feedback';

export type ButtonVariant = 'default' | 'primary' | 'ghost' | 'subtle' | 'danger' | 'danger-ghost';
export type ButtonSize = 'xs' | 'sm' | 'md' | 'lg';

const ICON_SIZE: Record<ButtonSize, number> = { xs: 12, sm: 13, md: 14, lg: 15 };

export const Button = forwardRef<
  HTMLButtonElement,
  ButtonHTMLAttributes<HTMLButtonElement> & {
    variant?: ButtonVariant;
    size?: ButtonSize;
    icon?: IconName;
    iconRight?: IconName;
    busy?: boolean;
    kbd?: string;
  }
>(function Button({ variant = 'default', size = 'md', icon, iconRight, busy, kbd, className, children, disabled, type, ...rest }, ref) {
  return (
    <button
      ref={ref}
      type={type ?? 'button'}
      className={`btn btn-${variant} btn-${size}${className ? ` ${className}` : ''}`}
      disabled={disabled || busy}
      aria-busy={busy || undefined}
      {...rest}
    >
      {busy ? <Spinner size={ICON_SIZE[size] - 2} /> : icon ? <Icon name={icon} size={ICON_SIZE[size]} /> : null}
      {children}
      {iconRight && <Icon name={iconRight} size={ICON_SIZE[size]} />}
      {kbd && <Kbd className="btn-kbd">{kbd}</Kbd>}
    </button>
  );
});

/** Icon-only button with an accessible label and a tooltip. */
export const IconButton = forwardRef<
  HTMLButtonElement,
  Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'children'> & {
    icon: IconName;
    label: string;
    variant?: ButtonVariant;
    size?: 'sm' | 'md';
    filled?: boolean;
    kbd?: string;
    active?: boolean;
  }
>(function IconButton({ icon, label, variant = 'ghost', size = 'sm', filled, kbd, active, className, type, ...rest }, ref) {
  return (
    <Tooltip.Root>
      <Tooltip.Trigger
        ref={ref}
        type={type ?? 'button'}
        aria-label={label}
        aria-pressed={active}
        className={`btn btn-${variant} btn-icon btn-${size}${active ? ' is-on' : ''}${className ? ` ${className}` : ''}`}
        {...rest}
      >
        <Icon name={icon} size={size === 'md' ? 16 : 15} fill={filled ? 'currentColor' : 'none'} />
      </Tooltip.Trigger>
      <Tooltip.Portal>
        <Tooltip.Positioner sideOffset={6}>
          <Tooltip.Popup className="tooltip">
            {label}
            {kbd && <Kbd>{kbd}</Kbd>}
          </Tooltip.Popup>
        </Tooltip.Positioner>
      </Tooltip.Portal>
    </Tooltip.Root>
  );
});

/** Tooltip around arbitrary content. */
export function Tip({ label, children, kbd, side }: { label: ReactNode; children: ReactNode; kbd?: string; side?: 'top' | 'bottom' | 'left' | 'right' }) {
  return (
    <Tooltip.Root>
      <Tooltip.Trigger render={<span className="tip-trigger" tabIndex={0} />}>{children}</Tooltip.Trigger>
      <Tooltip.Portal>
        <Tooltip.Positioner sideOffset={6} side={side}>
          <Tooltip.Popup className="tooltip">
            {label}
            {kbd && <Kbd>{kbd}</Kbd>}
          </Tooltip.Popup>
        </Tooltip.Positioner>
      </Tooltip.Portal>
    </Tooltip.Root>
  );
}
