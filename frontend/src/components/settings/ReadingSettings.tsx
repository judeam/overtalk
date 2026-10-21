'use client';
import { useEffect, useId } from 'react';
import { useWorkspacePreference } from '@/hooks/useWorkspacePreference';

export function ReadingSettings() {
  const [size, setSize] = useWorkspacePreference('reading-size', 'comfortable');
  const id = useId();
  useEffect(() => { document.documentElement.dataset.readingSize = size; }, [size]);
  return <div className="settings-reading-grid">
    <fieldset className="settings-reading-options"><legend>Text size</legend><p>Choose a comfortable size for transcripts and notes.</p>
      {([{ value: 'comfortable', label: 'Comfortable', note: 'More content in view' }, { value: 'large', label: 'Large', note: 'A little more breathing room' }, { value: 'extra-large', label: 'Extra large', note: 'Easier reading at a distance' }] as const).map(option => <label className="settings-reading-choice" key={option.value}><input type="radio" name={id} value={option.value} checked={size === option.value} onChange={() => setSize(option.value)} /><span><strong>{option.label}</strong><small>{option.note}</small></span></label>)}
    </fieldset>
    <div className="settings-reading-preview" data-size={size}><span className="settings-preview-label">Text preview</span><h4>A clearer view of the conversation</h4><p>Keep the important details close. Read the transcript, check the source, and pick up where you left off.</p><span className="settings-preview-source">Transcript · 02:14</span></div>
  </div>;
}
