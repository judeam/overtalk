import React from 'react';
import { Check } from 'lucide-react';
import { cn } from '@/lib/utils';
import type { OnboardingContainerProps } from '@/types/onboarding';
import { BrandMark } from '@/components/brand/BrandMark';
import { identity } from '@/lib/identity';

const LABELS: Record<number, string[]> = {
  3: ['Welcome', 'Speech model', 'Your sound'],
  4: ['Welcome', 'Speech model', 'Permissions', 'Your sound'],
};

/** First-run frame: brand, a labelled step bar, one question per screen. */
export function OnboardingContainer({
  title,
  description,
  children,
  step,
  totalSteps = 3,
  hideProgress = false,
  className,
}: OnboardingContainerProps) {
  const labels = LABELS[totalSteps] ?? Array.from({ length: totalSteps }, (_, i) => `Step ${i + 1}`);
  return (
    <div className="ot-setup">
      {!hideProgress && step && (
        <header className="ot-setup-head">
          <span className="ot-setup-brand"><BrandMark />{identity.name}</span>
          <ol className="ot-setup-steps" aria-label={`Setup, step ${step} of ${totalSteps}`}>
            {labels.map((label, i) => (
              <li key={label} aria-current={i + 1 === step ? 'step' : undefined} data-done={i + 1 < step ? 'true' : undefined}>
                <span className="ot-setup-dot">{i + 1 < step ? <Check aria-hidden="true" /> : i + 1}</span>
                <span className="ot-setup-label">{label}</span>
              </li>
            ))}
          </ol>
        </header>
      )}
      <main className={cn('ot-setup-main', className)}>
        {title && <h1>{title}</h1>}
        {description && <p className="ot-setup-lede">{description}</p>}
        <div className="ot-setup-body">{children}</div>
      </main>
    </div>
  );
}
