import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Lock, RefreshCw } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';

export const PRIVATEMODE_MODELS = ['glm-5.3', 'gpt-oss-120b', 'glm-5.3-flash'];
export const PRIVATEMODE_ENDPOINT = 'http://127.0.0.1:8080/v1';

export function PrivatemodeSettings({ endpoint, onEndpoint, apiKey, onKey, onModels }: {
  endpoint: string; onEndpoint: (value: string) => void;
  apiKey: string; onKey: (value: string) => void;
  onModels: (models: string[]) => void;
}) {
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState('');
  const [failed, setFailed] = useState(false);
  async function connect() {
    setBusy(true); setStatus(''); setFailed(false);
    try {
      const models = await invoke<{ name: string }[]>('api_get_privatemode_models', {
        endpoint: endpoint.trim() || PRIVATEMODE_ENDPOINT, apiKey: apiKey.trim() || null,
      });
      onModels(models.map(model => model.name));
      setStatus(`Connected · ${models.length} chat models available`);
    } catch (error) { setFailed(true); setStatus(String(error)); }
    finally { setBusy(false); }
  }
  return <section className="space-y-4 rounded-lg border border-teal-500/25 bg-teal-500/5 p-4" aria-label="Privatemode connection">
    <div className="flex items-start gap-3">
      <Lock className="mt-0.5 h-4 w-4 shrink-0 text-teal-600 dark:text-teal-300" aria-hidden="true" />
      <div className="space-y-1">
        <h3 className="text-sm font-semibold">Private cloud models</h3>
        <p className="text-sm text-muted-foreground">The official local proxy verifies and encrypts requests before they leave this device.</p>
      </div>
    </div>
    <div className="space-y-2">
      <Label htmlFor="privatemode-endpoint">Local proxy address</Label>
      <Input id="privatemode-endpoint" value={endpoint} placeholder={PRIVATEMODE_ENDPOINT}
        onChange={event => { onEndpoint(event.target.value); setStatus(''); }} />
    </div>
    <div className="space-y-2">
      <Label htmlFor="privatemode-key">API key · saved in your OS keychain</Label>
      <Input id="privatemode-key" type="password" autoComplete="off" value={apiKey}
        placeholder="Leave empty to use the saved key or proxy's key" onChange={event => onKey(event.target.value)} />
    </div>
    <div className="flex flex-wrap items-center gap-3">
      <Button type="button" variant="outline" disabled={busy} onClick={connect}>
        <RefreshCw className={`mr-2 h-4 w-4 ${busy ? 'animate-spin motion-reduce:animate-none' : ''}`} aria-hidden="true" />
        {busy ? 'Connecting…' : 'Connect & list models'}
      </Button>
      <a className="text-sm text-teal-700 underline underline-offset-4 dark:text-teal-300" href="https://docs.privatemode.ai/api/proxy-configuration/" target="_blank" rel="noreferrer">Proxy setup guide</a>
    </div>
    {status && <p role={failed ? 'alert' : 'status'} className={`text-sm ${failed ? 'text-destructive' : 'text-teal-700 dark:text-teal-300'}`}>{status}</p>}
  </section>;
}
