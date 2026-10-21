'use client';

import { Dialog, DialogContent, DialogTitle, DialogTrigger } from '@/components/ui/dialog';
import { VisuallyHidden } from '@/components/ui/visually-hidden';
import { About } from '@/components/brand/About';
import { BrandMark } from '@/components/brand/BrandMark';
import { identity } from '@/lib/identity';
import { CircleHelp, PanelLeft } from 'lucide-react';
import { useGuide } from '@/components/guide/GuideProvider';

export function AppHeader({ railOpen, onToggleRail }: { railOpen: boolean; onToggleRail: () => void }) {
  const guide = useGuide();
  return (
    <header className="ot-header">
      <button type="button" className="ot-icon-button ot-rail-toggle" aria-label={railOpen ? 'Hide meetings' : 'Show meetings'} aria-expanded={railOpen} onClick={onToggleRail}>
        <PanelLeft />
      </button>
      <Dialog aria-describedby={undefined}>
        <DialogTrigger asChild>
          <button type="button" className="ot-brand" aria-label={`About ${identity.name}`}>
            <BrandMark />
            <span className="ot-brand-name">{identity.name}</span>
          </button>
        </DialogTrigger>
        <DialogContent>
          <VisuallyHidden><DialogTitle>About {identity.name}</DialogTitle></VisuallyHidden>
          <About />
        </DialogContent>
      </Dialog>
      <span className="ot-tagline">{identity.description}</span>
      <button type="button" className="ot-guide-button" data-tour="guide" aria-label="Guide" title="Guide (F1)" aria-expanded={guide.isOpen} onClick={() => (guide.isOpen ? guide.close() : guide.open())}>
        <CircleHelp aria-hidden="true" /><span>Guide</span>
      </button>
    </header>
  );
}
