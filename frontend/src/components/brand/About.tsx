'use client';

import { useEffect, useState } from 'react';
import { getVersion } from '@tauri-apps/api/app';
import { invoke } from '@tauri-apps/api/core';
import { identity } from '@/lib/identity';
import { BrandMark } from './BrandMark';
import { Button } from '@/components/ui/button';

export function About() {
  const [version, setVersion] = useState('0.1.0');
  useEffect(() => { getVersion().then(setVersion).catch(() => {}); }, []);
  return (
    <div className="space-y-6 py-2">
      <div className="flex items-center gap-3"><BrandMark width={40} height={40} /><div><h2 className="text-2xl font-semibold">{identity.name}</h2><p className="text-sm text-muted-foreground">Version {version}</p></div></div>
      <p className="text-foreground">A local meeting notebook. Record conversations, review transcripts and speakers, then shape the notes you need.</p>
      <div className="space-y-3 text-sm text-muted-foreground">
        <p>Recordings and meetings are stored on your device. Cloud AI providers and external connections run only when you configure them.</p>
        <p>Telemetry and automatic updates are disabled in this independent build.</p>
        <p>{identity.name} is open source under the MIT license. The license and third-party notices are included with the application.</p>
      </div>
      <Button variant="outline" onClick={() => invoke('open_external_url', { url: identity.repository })}>Open repository</Button>
    </div>
  );
}
