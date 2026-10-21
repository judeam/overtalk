'use client';

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Button } from './ui/button';
import { Input } from './ui/input';
import { Switch } from './ui/switch';
import { SettingsRow } from './settings/SettingsSection';
import { toast } from 'sonner';

interface Settings {
  api_enabled: boolean;
  api_port: number;
  api_token: string;
  cli_token: string;
  mcp_token: string;
  record_control: boolean;
  write_notes: boolean;
  cli_enabled: boolean;
  mcp_enabled: boolean;
  read_meetings: boolean;
  read_transcripts: boolean;
  read_summaries: boolean;
  webhook_enabled: boolean;
  webhook_url: string;
  webhook_secret: string;
  webhook_include_summary: boolean;
}
interface Status { settings: Settings; config_path: string; companion_path: string }

export function IntegrationSettings() {
  const [status, setStatus] = useState<Status | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [hasUnsavedChanges, setHasUnsavedChanges] = useState(false);
  useEffect(() => {
    let active = true;
    invoke<Status>('integrations_get_settings').then(value => {
      if (active) setStatus({ ...value, settings: { ...value.settings, api_port: value.settings.api_port || 8421 } });
    })
      .catch(reason => { if (active) setError(String(reason)); });
    return () => { active = false; };
  }, []);
  if (!status) return <p role={error ? 'alert' : 'status'} className="p-4 text-sm">{error ?? 'Loading integration settings…'}</p>;
  const update = (patch: Partial<Settings>) => {
    setStatus({ ...status, settings: { ...status.settings, ...patch } });
    setHasUnsavedChanges(true);
  };
  const checkbox = (key: keyof Settings, label: string) => (
    <SettingsRow key={key} title={label} htmlFor={`connection-${key}`}>
      <Switch id={`connection-${key}`} disabled={saving} checked={Boolean(status.settings[key])} onCheckedChange={checked => update({ [key]: checked })} />
    </SettingsRow>
  );
  const mcpConfig = JSON.stringify({ mcpServers: { overtalk: { command: 'python3', args: [status.companion_path, '--config', status.config_path, 'mcp'] } } }, null, 2);
  const endpoint = `http://127.0.0.1:${status.settings.api_port || 8421}`;
  return (
    <div className="space-y-8">
      <div className="space-y-2">
        <h3 className="font-semibold">Connect your assistant and scripts</h3>
        <p className="text-sm text-gray-600">Access is off by default. Enable the clients you use, then grant their permissions below. Save changes to apply or revoke access.</p>
        {checkbox('cli_enabled', 'Enable command-line access')}
        {checkbox('mcp_enabled', 'Enable MCP assistant access')}
        {checkbox('api_enabled', 'Enable local HTTP API access')}
      </div>
      <fieldset className="settings-inline-panel space-y-3">
        <legend className="px-1 text-sm font-medium">Local API connection</legend>
        <label className="block text-sm">Port
          <Input type="number" min={1024} max={65535} step={1} value={status.settings.api_port || ''} disabled={saving} onChange={event => update({ api_port: Number(event.target.value) })} />
        </label>
        <p className="break-all text-sm"><span className="text-gray-600">Endpoint: </span><code>{endpoint}</code></p>
        <div>
          <label htmlFor="integration-api-token" className="block text-sm">API bearer token</label>
          <div className="mt-1 flex gap-2">
            <Input id="integration-api-token" type="password" readOnly value={status.settings.api_token} placeholder="Generated when permissions are saved" autoComplete="off" />
            <Button type="button" variant="outline" disabled={!status.settings.api_token || saving} onClick={async () => {
              try { await navigator.clipboard.writeText(status.settings.api_token); toast.success('API token copied'); }
              catch { toast.error('Could not copy the API token'); }
            }}>Copy</Button>
          </div>
        </div>
        <p className="text-xs text-gray-600">The listener binds only to this device at 127.0.0.1. It starts when API, MCP or CLI access is enabled and Overtalk is running. HTTP API clients use the token above; MCP and CLI use their own credentials from the connection file.</p>
      </fieldset>
      <fieldset className="settings-inline-panel">
        <legend className="px-1 text-sm font-medium">Read permissions</legend>
        {checkbox('read_meetings', 'Meeting titles and dates')}
        {checkbox('read_transcripts', 'Transcripts and speaker labels')}
        {checkbox('read_summaries', 'Summaries')}
        <p className="text-xs text-gray-600">These independent grants apply to every enabled API, MCP and CLI client. Your assistant may send the content it reads to its own provider.</p>
      </fieldset>
      <fieldset className="settings-inline-panel">
        <legend className="px-1 text-sm font-medium">Action permissions</legend>
        {checkbox('record_control', 'Allow starting, pausing, resuming and stopping recordings')}
        {checkbox('write_notes', 'Allow editing notes and generating summaries')}
        <p className="text-xs text-gray-600">Recording control and note changes require the Overtalk app to be running. MCP and CLI can read permitted saved content while the app is closed. Enabling a client does not grant these actions.</p>
      </fieldset>
      <div className="space-y-2">
        <h3 className="font-semibold">Summary webhooks</h3>
        {checkbox('webhook_enabled', 'Send a signed event when a new summary finishes')}
        <label className="block text-sm">Receiver URL
          <Input disabled={saving} value={status.settings.webhook_url} onChange={e => update({ webhook_url: e.target.value })} placeholder="https://your-server.example/overtalk" />
        </label>
        <label className="block text-sm">Signing secret
          <Input type="password" disabled={saving} value={status.settings.webhook_secret} onChange={e => update({ webhook_secret: e.target.value })} placeholder="Generated when you enable webhooks" autoComplete="off" />
        </label>
        {checkbox('webhook_include_summary', 'Include summary text in events')}
        <p className="text-xs text-gray-600">Events contain the meeting ID only unless you allow summary text. Verify the timestamp and HMAC-SHA256 signature at your receiver.</p>
      </div>
      {error && <p role="alert" className="text-sm text-red-700">{error}</p>}
      <Button disabled={saving} onClick={async () => {
        setSaving(true); setError(null);
        try {
          if (!Number.isInteger(status.settings.api_port) || status.settings.api_port < 1024 || status.settings.api_port > 65535) throw new Error('Enter a port from 1024 to 65535.');
          setStatus(await invoke<Status>('integrations_save_settings', { settings: status.settings }));
          setHasUnsavedChanges(false); toast.success('Integration permissions saved');
        }
        catch (reason) { setError(String(reason)); }
        finally { setSaving(false); }
      }}>{saving ? 'Saving…' : 'Save permissions'}</Button>
      {hasUnsavedChanges && <p role="status" className="text-sm text-gray-600">Changes will apply when you save permissions.</p>}
      <details className="settings-advanced text-sm">
        <summary className="cursor-pointer font-medium">Connection instructions</summary>
        <p className="mt-2">Add this to your assistant’s MCP configuration. Use your Python executable on Windows.</p>
        <pre className="overflow-x-auto rounded-md bg-gray-100 p-3 text-xs">{mcpConfig}</pre>
        <p>For scripts:</p>
        <pre className="overflow-x-auto rounded-md bg-gray-100 p-3 text-xs">{`python3 ${JSON.stringify(status.companion_path)} --config ${JSON.stringify(status.config_path)} list`}</pre>
        <p>The companion reads its credentials from the saved private configuration file. Keep that file private. The HTTP API accepts <code>Authorization: Bearer &lt;token&gt;</code> with <code>GET /health</code> or JSON requests to <code>POST /v1/call</code>.</p>
      </details>
    </div>
  );
}
