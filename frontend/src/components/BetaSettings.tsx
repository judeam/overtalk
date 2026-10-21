"use client";
import { Switch } from './ui/switch';
import { Info } from 'lucide-react';
import { useConfig } from '@/contexts/ConfigContext';
import { BetaFeatureKey, BETA_FEATURE_NAMES, BETA_FEATURE_DESCRIPTIONS } from '@/types/betaFeatures';
import { SettingsSection, SettingsRow } from '@/components/settings/SettingsSection';
export function BetaSettings() {
  const { betaFeatures, toggleBetaFeature } = useConfig();
  const featureOrder: BetaFeatureKey[] = ['importAndRetranscribe'];
  return <div>
    <SettingsSection title="Experimental features" description="These features may have rough edges. Your existing meetings are unaffected when you turn them off.">
      {featureOrder.map(key => <SettingsRow key={key} title={BETA_FEATURE_NAMES[key]} description={BETA_FEATURE_DESCRIPTIONS[key]} htmlFor={`beta-${key}`}>
        <Switch id={`beta-${key}`} checked={betaFeatures[key]} onCheckedChange={checked => toggleBetaFeature(key, checked)} />
      </SettingsRow>)}
    </SettingsSection>
    <p className="settings-note"><Info />Disabled features are hidden from the workspace.</p>
  </div>;
}
