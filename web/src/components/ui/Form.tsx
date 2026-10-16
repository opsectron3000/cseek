import { forwardRef, useId, useState, type InputHTMLAttributes, type ReactNode, type TextareaHTMLAttributes } from 'react';
import { Switch as BaseSwitch } from '@base-ui/react/switch';
import { Select as BaseSelect } from '@base-ui/react/select';
import { Checkbox as BaseCheckbox } from '@base-ui/react/checkbox';
import { Icon, type IconName } from '../Icon';
import { Button } from './Button';
import { Kbd } from './Feedback';

type InputSize = 'sm' | 'md' | 'lg';

/** Text input with optional leading icon, trailing kbd hint / clear button / custom slot. */
export const Input = forwardRef<
  HTMLInputElement,
  Omit<InputHTMLAttributes<HTMLInputElement>, 'size'> & {
    icon?: IconName;
    kbd?: string;
    onClear?: () => void;
    trailing?: ReactNode;
    inputSize?: InputSize;
  }
>(function Input({ icon, kbd, onClear, trailing, inputSize = 'md', className, ...rest }, ref) {
  const cls = `input${inputSize !== 'md' ? ` input-${inputSize}` : ''}${className ? ` ${className}` : ''}`;
  const showClear = onClear && rest.value !== undefined && String(rest.value).length > 0;
  const trail = trailing || showClear || kbd;
  if (!icon && !trail) return <input ref={ref} className={cls} {...rest} />;
  return (
    <div className={`input-wrap${icon ? ' has-lead' : ''}${trail ? ' has-trail' : ''}`}>
      {icon && (
        <span className="input-lead">
          <Icon name={icon} size={inputSize === 'lg' ? 16 : 14} />
        </span>
      )}
      <input ref={ref} className={cls} {...rest} />
      {trail && (
        <span className="input-trail">
          {trailing}
          {showClear ? (
            <button type="button" className="btn btn-ghost btn-icon btn-xs" style={{ width: 20, height: 20 }} aria-label="Clear" onClick={onClear}>
              <Icon name="x" size={12} />
            </button>
          ) : (
            kbd && <Kbd>{kbd}</Kbd>
          )}
        </span>
      )}
    </div>
  );
});

export const Textarea = forwardRef<HTMLTextAreaElement, TextareaHTMLAttributes<HTMLTextAreaElement>>(function Textarea(
  { className, ...rest },
  ref,
) {
  return <textarea ref={ref} className={`input${className ? ` ${className}` : ''}`} {...rest} />;
});

/** Label + control + hint. Pass a render function to get the generated id. */
export function Field({
  label,
  hint,
  children,
  className,
}: {
  label: ReactNode;
  hint?: ReactNode;
  children: ReactNode | ((id: string) => ReactNode);
  className?: string;
}) {
  const id = useId();
  return (
    <div className={`field${className ? ` ${className}` : ''}`}>
      <label className="field-label" htmlFor={id}>
        {label}
      </label>
      {typeof children === 'function' ? children(id) : children}
      {hint && <span className="field-hint">{hint}</span>}
    </div>
  );
}

export function Checkbox({
  checked,
  onChange,
  label,
  ariaLabel,
  indeterminate,
  disabled,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label?: ReactNode;
  ariaLabel?: string;
  indeterminate?: boolean;
  disabled?: boolean;
}) {
  const box = (
    <BaseCheckbox.Root
      className="checkbox"
      checked={checked}
      indeterminate={indeterminate}
      disabled={disabled}
      onCheckedChange={(v) => onChange(v)}
      aria-label={ariaLabel ?? (typeof label === 'string' ? label : undefined)}
    >
      <BaseCheckbox.Indicator className="checkbox-indicator">
        <Icon name={indeterminate ? 'more' : 'check'} size={11} strokeWidth={2.2} />
      </BaseCheckbox.Indicator>
    </BaseCheckbox.Root>
  );
  if (!label) return box;
  return (
    <label className="check-label">
      {box}
      <span>{label}</span>
    </label>
  );
}

export function Switch({
  checked,
  onChange,
  label,
  ariaLabel,
  disabled,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label?: ReactNode;
  ariaLabel?: string;
  disabled?: boolean;
}) {
  const sw = (
    <BaseSwitch.Root
      className="switch"
      checked={checked}
      onCheckedChange={(v) => onChange(v)}
      disabled={disabled}
      aria-label={ariaLabel ?? (typeof label === 'string' ? label : undefined)}
    >
      <BaseSwitch.Thumb className="switch-thumb" />
    </BaseSwitch.Root>
  );
  if (!label) return sw;
  return (
    <label className="switch-label">
      {sw}
      <span>{label}</span>
    </label>
  );
}

export interface SelectOption<V extends string> {
  value: V;
  label: ReactNode;
}

export function Select<V extends string>({
  value,
  onChange,
  options,
  ariaLabel,
  className,
}: {
  value: V;
  onChange: (v: V) => void;
  options: SelectOption<V>[];
  ariaLabel: string;
  className?: string;
}) {
  return (
    <BaseSelect.Root items={options} value={value} onValueChange={(v) => v !== null && onChange(v as V)}>
      <BaseSelect.Trigger className={`select-trigger${className ? ` ${className}` : ''}`} aria-label={ariaLabel}>
        <BaseSelect.Value />
        <BaseSelect.Icon className="select-icon">
          <Icon name="chevronDown" size={12} />
        </BaseSelect.Icon>
      </BaseSelect.Trigger>
      <BaseSelect.Portal>
        <BaseSelect.Positioner className="popover-positioner" sideOffset={4} alignItemWithTrigger={false}>
          <BaseSelect.Popup className="menu-popup">
            {options.map((o) => (
              <BaseSelect.Item key={o.value} value={o.value} className="menu-item">
                <BaseSelect.ItemIndicator className="menu-check" keepMounted>
                  <Icon name="check" size={12} />
                </BaseSelect.ItemIndicator>
                <BaseSelect.ItemText>{o.label}</BaseSelect.ItemText>
              </BaseSelect.Item>
            ))}
          </BaseSelect.Popup>
        </BaseSelect.Positioner>
      </BaseSelect.Portal>
    </BaseSelect.Root>
  );
}

export interface Segment<V extends string> {
  value: V;
  label: ReactNode;
  icon?: IconName;
  count?: number;
}

/** iOS-style segmented control; the selection pill glides between options. */
export function SegmentedControl<V extends string>({
  value,
  onChange,
  options,
  ariaLabel,
}: {
  value: V;
  onChange: (v: V) => void;
  options: Segment<V>[];
  ariaLabel: string;
}) {
  return (
    <div className="segmented" role="radiogroup" aria-label={ariaLabel}>
      {options.map((o) => {
        const on = o.value === value;
        return (
          <button
            key={o.value}
            type="button"
            role="radio"
            aria-checked={on}
            className="segment"
            onClick={() => onChange(o.value)}
            onKeyDown={(e) => {
              if (e.key !== 'ArrowRight' && e.key !== 'ArrowLeft') return;
              e.preventDefault();
              const i = options.findIndex((x) => x.value === value);
              const n = options[(i + (e.key === 'ArrowRight' ? 1 : -1) + options.length) % options.length];
              onChange(n.value);
              (e.currentTarget.parentElement?.querySelector(`[data-v="${CSS.escape(n.value)}"]`) as HTMLElement | null)?.focus();
            }}
            data-v={o.value}
            tabIndex={on ? 0 : -1}
          >
            {on && <span className="segment-pill" />}
            {o.icon && <Icon name={o.icon} size={13} />}
            <span>{o.label}</span>
            {o.count !== undefined && o.count > 0 && <span className="count">{o.count}</span>}
          </button>
        );
      })}
    </div>
  );
}

/** Editable list of strings (chips + add input). */
export function StringList({
  value,
  onChange,
  placeholder = 'Add…',
  ariaLabel,
}: {
  value: string[];
  onChange: (v: string[]) => void;
  placeholder?: string;
  ariaLabel?: string;
}) {
  const [draft, setDraft] = useState('');
  const add = () => {
    const v = draft.trim();
    if (!v) return;
    if (!value.includes(v)) onChange([...value, v]);
    setDraft('');
  };
  return (
    <div className="string-list">
      {value.length > 0 && (
        <ul className="chips">
          {value.map((v, i) => (
            <li key={`${v}${i}`} className="chip">
              <span className="chip-text">{v}</span>
              <button type="button" className="chip-x" aria-label={`Remove ${v}`} onClick={() => onChange(value.filter((_, j) => j !== i))}>
                <Icon name="x" size={10} />
              </button>
            </li>
          ))}
        </ul>
      )}
      <div className="row gap-xs">
        <input
          className="input input-sm"
          value={draft}
          placeholder={placeholder}
          aria-label={ariaLabel ?? placeholder}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              e.preventDefault();
              add();
            }
          }}
        />
        <Button size="sm" icon="plus" onClick={add} disabled={!draft.trim()}>
          Add
        </Button>
      </div>
    </div>
  );
}
