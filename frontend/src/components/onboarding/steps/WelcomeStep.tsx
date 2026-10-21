import React from 'react';
import { OnboardingContainer } from '../OnboardingContainer';
import { useOnboarding } from '@/contexts/OnboardingContext';
import { BrandMark } from '@/components/brand/BrandMark';
import { identity } from '@/lib/identity';

// A small session drawn from the app's own parts: three voices, one decision, the playhead.
const LANES = [
  { tone: 'teal', clips: [[8, 22], [44, 62], [80, 94]] },
  { tone: 'magenta', clips: [[23, 40], [64, 79]] },
  { tone: 'slate', clips: [[34, 47], [70, 84]] },
] as const;

export function WelcomeStep() {
  const { goNext } = useOnboarding();

  return (
    <OnboardingContainer title="" step={1} hideProgress={true}>
      <div className="ot-welcome">
        <div className="ot-welcome-brand">
          <BrandMark />
          <span>{identity.name}</span>
        </div>
        <h1>See who said what, and when it was decided.</h1>
        <p className="ot-welcome-lede">
          Record a meeting and every voice gets its own lane. Decisions and actions sit on the same timeline, so one click plays the moment they were said.
        </p>
        <div className="ot-welcome-session" aria-hidden="true">
          {LANES.map((lane) => (
            <div key={lane.tone} className={`ot-welcome-lane ot-tone-${lane.tone}`}>
              <span className="ot-chip" />
              <span className="ot-welcome-track">
                {lane.clips.map(([from, to]) => <i key={from} style={{ left: `${from}%`, width: `${to - from}%` }} />)}
              </span>
            </div>
          ))}
          <div className="ot-welcome-lane ot-tone-decision">
            <span className="ot-chip" />
            <span className="ot-welcome-track"><b style={{ left: '44%' }}>Decision</b></span>
          </div>
          <span className="ot-welcome-playhead" />
        </div>
        <button type="button" className="ot-button ot-button-primary ot-welcome-start" onClick={goNext}>Get started</button>
        <p className="ot-welcome-note">
          Recordings and transcripts are stored on this computer. Setup downloads the speech model once; cloud AI is used only when you choose a cloud provider.
        </p>
      </div>
    </OnboardingContainer>
  );
}
