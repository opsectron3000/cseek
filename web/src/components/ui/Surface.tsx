import type { ReactNode } from 'react';
import { Menu } from '@base-ui/react/menu';
import { Tabs as BaseTabs } from '@base-ui/react/tabs';
import { Icon, type IconName } from '../Icon';
import { Count } from './Feedback';

export function PageHeader({ title, sub, children }: { title: ReactNode; sub?: ReactNode; children?: ReactNode }) {
  return (
    <header className="page-header">
      <div className="page-title">
        <h1>{title}</h1>
        {sub !== undefined && sub !== null && sub !== false && <span className="page-sub">{sub}</span>}
      </div>
      {children && <div className="page-actions">{children}</div>}
    </header>
  );
}

export function Card({
  title,
  actions,
  children,
  className,
}: {
  title?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section className={`card${className ? ` ${className}` : ''}`}>
      {(title || actions) && (
        <div className="card-head">
          {title && <h2 className="card-title">{title}</h2>}
          {actions}
        </div>
      )}
      {children}
    </section>
  );
}

export function Panel({
  title,
  actions,
  children,
  className,
}: {
  title?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section className={`panel${className ? ` ${className}` : ''}`}>
      {(title || actions) && (
        <div className="panel-head">
          <div className="panel-title">{title}</div>
          {actions}
        </div>
      )}
      {children}
    </section>
  );
}

export function Kpi({ label, value, sub }: { label: string; value: ReactNode; sub?: ReactNode }) {
  return (
    <div className="kpi">
      <div className="kpi-label">{label}</div>
      <div className="kpi-value">{value}</div>
      {sub && <div className="kpi-sub">{sub}</div>}
    </div>
  );
}

export interface TabDef<V extends string> {
  value: V;
  label: ReactNode;
  icon?: IconName;
  count?: number;
}

/** Underline tabs with a sliding indicator; panels are rendered by the caller. */
export function Tabs<V extends string>({
  value,
  onChange,
  tabs,
  ariaLabel,
  children,
}: {
  value: V;
  onChange: (v: V) => void;
  tabs: TabDef<V>[];
  ariaLabel: string;
  children?: ReactNode;
}) {
  return (
    <BaseTabs.Root value={value} onValueChange={(v) => onChange(v as V)} className="tabs">
      <BaseTabs.List className="tab-list" aria-label={ariaLabel}>
        {tabs.map((t) => (
          <BaseTabs.Tab key={t.value} value={t.value} className="tab">
            {t.icon && <Icon name={t.icon} size={14} />}
            {t.label}
            {t.count !== undefined && <Count n={t.count} />}
          </BaseTabs.Tab>
        ))}
        <BaseTabs.Indicator className="tab-indicator" />
      </BaseTabs.List>
      {children}
    </BaseTabs.Root>
  );
}

export const TabPanel = ({ value, children }: { value: string; children: ReactNode }) => (
  <BaseTabs.Panel value={value} className="tab-panel">
    {children}
  </BaseTabs.Panel>
);

export interface MenuAction {
  label: string;
  onSelect: () => void;
  icon?: IconName;
  danger?: boolean;
  disabled?: boolean;
  hidden?: boolean;
}

/** Overflow "…" menu built on Base UI Menu. */
export function ActionMenu({
  actions,
  label = 'More actions',
  trigger,
}: {
  actions: (MenuAction | 'separator')[];
  label?: string;
  trigger?: ReactNode;
}) {
  const visible = actions.filter((a) => a === 'separator' || !a.hidden);
  return (
    <Menu.Root>
      <Menu.Trigger className={trigger ? 'btn btn-default btn-sm' : 'btn btn-ghost btn-icon btn-sm'} aria-label={label}>
        {trigger ?? <Icon name="more" size={15} strokeWidth={2.4} />}
      </Menu.Trigger>
      <Menu.Portal>
        <Menu.Positioner className="popover-positioner" sideOffset={4} align="end">
          <Menu.Popup className="menu-popup">
            {visible.map((a, i) =>
              a === 'separator' ? (
                <Menu.Separator key={`sep${i}`} className="menu-sep" />
              ) : (
                <Menu.Item
                  key={a.label}
                  className={`menu-item${a.danger ? ' menu-item-danger' : ''}`}
                  disabled={a.disabled}
                  onClick={a.onSelect}
                >
                  {a.icon && <Icon name={a.icon} size={14} />}
                  {a.label}
                </Menu.Item>
              ),
            )}
          </Menu.Popup>
        </Menu.Positioner>
      </Menu.Portal>
    </Menu.Root>
  );
}
