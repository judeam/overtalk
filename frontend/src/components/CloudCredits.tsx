'use client';

import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';

type Account = {
  configured: boolean;
  connected: boolean;
  balance?: { available_micro_usd: number; held_micro_usd: number; suspended: boolean };
};
const dollars = (units: number) => new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD' }).format(units / 1_000_000);

export function CloudCredits({ onUseCloud }: { onUseCloud: () => void }) {
  const [account, setAccount] = useState<Account>({ configured: false, connected: false });
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState('');
  const [restoring, setRestoring] = useState(false);
  const [recovery, setRecovery] = useState('');
  const mounted = useRef(true);
  const inFlight = useRef(false);
  async function refresh() {
    if (inFlight.current) return;
    inFlight.current = true;
    setBusy(true);
    try {
      const result = await invoke<Account>('cloud_account', { action: { op: 'status' } });
      if (mounted.current) { setAccount(result); setMessage(''); }
    } catch (error) { if (mounted.current) setMessage(String(error)); }
    finally { inFlight.current = false; if (mounted.current) setBusy(false); }
  }
  useEffect(() => { mounted.current = true; void refresh(); return () => { mounted.current = false; }; }, []);
  async function checkout(plan?: string) {
    if (inFlight.current) return;
    inFlight.current = true;
    setBusy(true); setMessage('');
    try {
      const result = await invoke<{ url: string }>('cloud_account', { action: plan ? { op: 'checkout', plan } : { op: 'billing' } });
      if (!mounted.current) return;
      await invoke('open_external_url', { url: result.url });
      if (mounted.current) setMessage('Finish in your browser, then refresh your credits. Opening checkout does not add credits.');
    } catch (error) { if (mounted.current) setMessage(String(error)); }
    finally { inFlight.current = false; if (mounted.current) setBusy(false); }
  }
  async function restore() {
    if (inFlight.current) return;
    inFlight.current = true; setBusy(true);
    try {
      const result = await invoke<Account>('cloud_account', { action: { op: 'restore', account_token: recovery.trim() } });
      if (mounted.current) { setAccount(result); setMessage('Cloud access restored.'); setRestoring(false); }
    } catch (error) { if (mounted.current) setMessage(String(error)); }
    finally { inFlight.current = false; if (mounted.current) { setBusy(false); setRecovery(''); } }
  }
  return <section className="space-y-4 rounded-lg border border-border bg-card p-5" aria-label="Cloud credits">
    <div className="flex flex-wrap items-center justify-between gap-3">
      <h3 className="text-base font-semibold">Cloud credits</h3>
      <Button type="button" variant="ghost" size="sm" disabled={busy} onClick={() => void refresh()}>{busy ? 'Checking…' : 'Refresh'}</Button>
    </div>
    <p className="font-medium text-teal-700 dark:text-teal-300">{account.connected ? `${dollars(account.balance?.available_micro_usd ?? 0)} available` : 'Local processing is always available'}</p>
    {!account.configured && <p className="text-sm text-muted-foreground">Cloud plans are not available yet. Local models and your own provider connections keep working.</p>}
    {account.balance?.suspended && <p role="status" className="text-sm">Cloud spending is paused for a billing review.</p>}
    {!!account.balance?.held_micro_usd && <p role="status" className="text-sm">{dollars(account.balance.held_micro_usd)} reserved for pending requests. If this remains after generation, contact billing support.</p>}
    <p className="text-sm text-muted-foreground">One-time includes one $2 starter grant. Monthly adds $2 after each paid month; unused monthly credits expire. No automatic top-ups.</p>
    <div className="flex flex-wrap gap-2">
      <Button type="button" variant="outline" disabled={!account.configured || busy} onClick={() => void checkout('summaries_once')}>One-time</Button>
      <Button type="button" variant="outline" disabled={!account.configured || busy} onClick={() => void checkout('summaries_monthly')}>Monthly</Button>
      {account.connected && <>
        <Button type="button" disabled={busy || account.balance?.suspended || !account.balance?.available_micro_usd} onClick={onUseCloud}>Use cloud for notes</Button>
        <Button type="button" variant="outline" disabled={busy} onClick={() => void checkout('credits')}>Add credits</Button>
        <Button type="button" variant="ghost" disabled={busy} onClick={() => void checkout()}>Billing</Button>
      </>}
    </div>
    <p className="text-sm text-muted-foreground">Cloud notes send the transcript through Overtalk to Privatemode. To keep encryption on your device, choose your own Privatemode connection instead.</p>
    {account.configured && <Button type="button" variant="ghost" onClick={() => setRestoring(!restoring)}>{restoring ? 'Close recovery' : 'Restore existing access'}</Button>}
    {restoring && <div className="space-y-2">
      <label htmlFor="cloud-recovery" className="text-sm">Private recovery code</label>
      <Input id="cloud-recovery" type="password" autoComplete="off" value={recovery} onChange={event => setRecovery(event.target.value)} />
      <p className="text-sm text-muted-foreground">This code gives access to your cloud account and billing. Keep it private.</p>
      <Button type="button" disabled={busy || !recovery.trim()} onClick={() => void restore()}>Restore access</Button>
    </div>}
    {message && <p role="status" className="text-sm">{message}</p>}
  </section>;
}
