'use client';

import * as Primitive from '@radix-ui/react-dropdown-menu';
import type { ReactNode } from 'react';

/** Overtalk's menu: Radix behaviour, Overtalk surfaces. */
export function Menu({ trigger, children, align = 'end' }: { trigger: ReactNode; children: ReactNode; align?: 'start' | 'end' | 'center' }) {
  return (
    <Primitive.Root modal={false}>
      <Primitive.Trigger asChild>{trigger}</Primitive.Trigger>
      <Primitive.Portal>
        <Primitive.Content className="ot-menu" align={align} sideOffset={6} collisionPadding={12}>
          {children}
        </Primitive.Content>
      </Primitive.Portal>
    </Primitive.Root>
  );
}

export function MenuItem({ icon, children, onSelect, danger, disabled }: { icon?: ReactNode; children: ReactNode; onSelect: () => void; danger?: boolean; disabled?: boolean }) {
  return (
    <Primitive.Item className="ot-menu-item" data-danger={danger ? 'true' : undefined} disabled={disabled} onSelect={onSelect}>
      {icon}
      <span>{children}</span>
    </Primitive.Item>
  );
}

export function MenuSeparator() {
  return <Primitive.Separator className="ot-menu-sep" />;
}
