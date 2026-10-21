import { ReactNode, useId } from 'react';
import { cn } from '@/lib/utils';

export function SettingsSection({ title, description, children, className }: {
  title: string; description?: string; children: ReactNode; className?: string;
}) {
  const id = useId();
  return <section aria-labelledby={id} className={cn('settings-section', className)}>
    <header className="settings-section-heading">
      <h3 id={id}>{title}</h3>
      {description && <p>{description}</p>}
    </header>
    <div className="settings-section-body">{children}</div>
  </section>;
}

export function SettingsRow({ title, description, children, htmlFor }: {
  title: string; description?: string; children: ReactNode; htmlFor?: string;
}) {
  return <div className="settings-row">
    <div className="settings-row-copy">
      <label htmlFor={htmlFor} className="settings-row-title">{title}</label>
      {description && <p>{description}</p>}
    </div>
    <div className="settings-row-control">{children}</div>
  </div>;
}

export function SettingsLoading() {
  return <div role="status" aria-label="Loading settings" className="settings-loading">
    <div /><div /><div />
    <span className="sr-only">Loading settings…</span>
  </div>;
}
