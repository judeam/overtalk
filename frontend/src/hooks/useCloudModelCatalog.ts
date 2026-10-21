import { useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

const catalogs = {
  openai: {
    command: 'get_openai_models',
    fallback: ['gpt-4o', 'gpt-4o-mini', 'gpt-4-turbo', 'gpt-4', 'gpt-3.5-turbo', 'o1', 'o1-mini', 'o3', 'o3-mini'],
  },
  claude: {
    command: 'get_anthropic_models',
    fallback: ['claude-sonnet-4-5-20250929', 'claude-haiku-4-5-20251001', 'claude-opus-4-5-20251101', 'claude-3-5-sonnet-latest'],
  },
  groq: {
    command: 'get_groq_models',
    fallback: ['llama-3.3-70b-versatile', 'llama-3.1-70b-versatile', 'mixtral-8x7b-32768', 'gemma2-9b-it'],
  },
};

/** Discover cloud models through the service; late replies cannot replace a newer selection. */
export function useCloudModelCatalog(provider: keyof typeof catalogs, apiKey: string | null, enabled: boolean) {
  // Opaque identity prevents even one render of the previous account's result, without caching keys.
  const request = useMemo(() => ({}), [provider, apiKey, enabled]);
  const [result, setResult] = useState<{ request: object; models: string[]; loading: boolean } | null>(null);
  const catalog = catalogs[provider];

  useEffect(() => {
    if (!enabled || !apiKey?.trim()) return;
    let active = true;
    setResult({ request, models: [], loading: true });
    invoke<{ id: string }[]>(catalog.command, { apiKey }).then(
      models => { if (active) setResult({ request, models: models.map(model => model.id), loading: false }); },
      () => { if (active) setResult({ request, models: [], loading: false }); },
    );
    return () => { active = false; };
  }, [request, catalog.command, apiKey, enabled]);

  const current = result?.request === request ? result : null;
  return {
    models: current?.models.length ? current.models : catalog.fallback,
    loading: enabled && !!apiKey?.trim() && (current?.loading ?? true),
  };
}
