'use client';

import { lazy, Suspense, useState, useEffect, useRef } from 'react';
import { BackToWorkspace } from '@/components/settings/BackToWorkspace';
import { Search, Check, Settings2, Mic, AudioLines, FileText, FlaskConical, Plug } from 'lucide-react';
import { useRouter, useSearchParams } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { PreferenceSettings } from '@/components/PreferenceSettings';
import { useConfig } from '@/contexts/ConfigContext';
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs';

const TranscriptSettings = lazy(() => import('@/components/TranscriptSettings').then(module => ({ default: module.TranscriptSettings })));
const RecordingSettings = lazy(() => import('@/components/RecordingSettings').then(module => ({ default: module.RecordingSettings })));
const SummaryModelSettings = lazy(() => import('@/components/SummaryModelSettings').then(module => ({ default: module.SummaryModelSettings })));
const IntegrationSettings = lazy(() => import('@/components/IntegrationSettings').then(module => ({ default: module.IntegrationSettings })));
const LibrarySettings = lazy(() => import('@/components/LibrarySettings').then(module => ({ default: module.LibrarySettings })));
const BetaSettings = lazy(() => import('@/components/BetaSettings').then(module => ({ default: module.BetaSettings })));

const TABS = [
  { value: 'general', label: 'General', icon: Settings2, description: 'Make the workspace comfortable, choose what is saved, and control notifications.' },
  { value: 'recording', label: 'Recordings', icon: Mic, description: 'Choose your audio sources and how recordings are saved.' },
  { value: 'Transcriptionmodels', label: 'Transcription', icon: AudioLines, description: 'Choose the engine that turns your audio into a transcript.' },
  { value: 'summaryModels', label: 'Summary', icon: FileText, description: 'Set the language and model used to turn transcripts into notes.' },
  { value: 'library', label: 'Library & backups', icon: FileText, description: 'Semantic search, encrypted backups and local runtime diagnostics.' },
  { value: 'connections', label: 'Connections', icon: Plug, description: 'Give your assistant and scripts deliberate, revocable access.' },
  { value: 'beta', label: 'Beta', icon: FlaskConical, description: 'Try features that are still being refined.' },
] as const;

function SettingsContent() {
  const searchParams = useSearchParams();
  const [visited, setVisited] = useState<string[]>(["general"]);
  const router = useRouter();
  const { transcriptModelConfig, setTranscriptModelConfig } = useConfig();
  const [activeTab, setActiveTab] = useState('general');
  const [compact, setCompact] = useState(false);
  const [filter, setFilter] = useState('');
  const keywords: Record<string, string> = { general: 'reading text size display notifications privacy storage folder', recording: 'microphone sound audio sources save capture system', Transcriptionmodels: 'speech voice transcription model engine language gpu cuda', summaryModels: 'notes ai summary model provider api key', library: 'search index semantic backup restore encryption', connections: 'integration api webhook cli mcp assistant access', beta: 'experimental features' };
  const shownTabs = TABS.filter(tab => `${tab.label} ${tab.description} ${keywords[tab.value]}`.toLowerCase().includes(filter.trim().toLowerCase()));
  const categoryRail = useRef<HTMLDivElement>(null);
  useEffect(() => {

    const query = window.matchMedia('(max-width: 767px)');
    const update = () => setCompact(query.matches);
    update(); query.addEventListener('change', update);
    return () => query.removeEventListener('change', update);
  }, []);
  useEffect(() => {
    const requested = searchParams.get('tab');
    if (requested && TABS.some(tab => tab.value === requested)) setActiveTab(requested);
  }, [searchParams]);
  useEffect(() => { setVisited(values => values.includes(activeTab) ? values : [...values, activeTab]); }, [activeTab]);
  useEffect(() => {
    if (!compact) return;
    const rail = categoryRail.current;
    const selected = rail?.querySelector<HTMLElement>('[role="tab"][data-state="active"]');
    if (rail && selected) rail.scrollLeft += selected.getBoundingClientRect().left - rail.getBoundingClientRect().left - (rail.clientWidth-selected.offsetWidth)/2;
  }, [activeTab, compact]);
  useEffect(() => {
    let active = true;
    invoke<typeof transcriptModelConfig>('api_get_transcript_config').then(config => {
      if (active && config) setTranscriptModelConfig({
        provider: config.provider || 'localWhisper', model: config.model || 'large-v3', apiKey: config.apiKey || null,
      });
    }).catch(error => console.error('Failed to load transcript config:', error));
    return () => { active = false; };
  }, [setTranscriptModelConfig]);
  return <div className="settings-page custom-scrollbar" data-section={activeTab}>
    <div className="settings-shell">
      <BackToWorkspace />
      <header className="settings-header"><div><h1>Settings</h1><p>Your workspace, the way you work.</p></div></header>
      <Tabs value={activeTab} onValueChange={value => { setFilter(''); setActiveTab(value); const query = new URLSearchParams(searchParams.toString()); query.set("tab", value); router.replace(`/settings?${query}`); }} className="settings-layout" orientation={compact ? "horizontal" : "vertical"}>
        <aside className="settings-sidebar">
        <label className="settings-find"><Search size={18} aria-hidden /><input aria-label="Find a setting" placeholder="Find a setting…" value={filter} onChange={event => setFilter(event.target.value)} onKeyDown={event => { if (event.key === 'Escape') setFilter(''); if (event.key === 'Enter' && shownTabs[0]) { const next = shownTabs[0].value; setActiveTab(next); setFilter(''); const query = new URLSearchParams(searchParams.toString()); query.set('tab', next); router.replace(`/settings?${query}`); } }} /></label>
        <TabsList ref={categoryRail} data-tour="settings-nav" aria-label="Settings categories" className="settings-navigation flex w-full min-w-0 h-auto items-stretch justify-start bg-transparent p-0">
          {shownTabs.map(tab => <TabsTrigger key={tab.value} value={tab.value} className="settings-tab justify-start transition-colors data-[state=active]:bg-transparent">
            <tab.icon size={19} /><span><strong>{tab.label}</strong><small>{tab.value === 'general' ? 'Display, alerts & storage' : tab.value === 'recording' ? 'Sources & saved audio' : tab.value === 'Transcriptionmodels' ? 'Speech & languages' : tab.value === 'summaryModels' ? 'Notes & AI providers' : tab.value === 'library' ? 'Search & recovery' : tab.value === 'connections' ? 'Apps & access' : 'Early features'}</small></span>
          </TabsTrigger>)}
        </TabsList>
        {!shownTabs.length && <p className="settings-find-empty">No matching settings. Try “audio”, “notes” or “storage”.</p>}
        </aside>
        <div className="settings-content-area min-w-0">
          {TABS.filter(tab => visited.includes(tab.value) || tab.value === activeTab).map(tab => <TabsContent forceMount hidden={activeTab !== tab.value} key={tab.value} value={tab.value} className="settings-content" data-tour="settings-body">
            <header className="settings-content-header"><h2>{tab.label}</h2><p>{tab.description}</p><p className="ot-settings-save-hint">{["general", "recording", "beta"].includes(tab.value) ? <><Check size={15} />Changes save automatically</> : "Save changes in each section. Your draft stays here when you switch categories."}</p></header>
            <Suspense fallback={<p role="status">Loading settings…</p>}>
            {tab.value === 'general' && <PreferenceSettings />}
            {tab.value === 'recording' && <RecordingSettings />}
            {tab.value === 'Transcriptionmodels' && <TranscriptSettings transcriptModelConfig={transcriptModelConfig} setTranscriptModelConfig={setTranscriptModelConfig} />}
            {tab.value === 'summaryModels' && <SummaryModelSettings />}
            {tab.value === 'library' && <LibrarySettings />}
            {tab.value === 'connections' && <IntegrationSettings />}
            {tab.value === 'beta' && <BetaSettings />}
            </Suspense>
          </TabsContent>)}
        </div>
      </Tabs>
    </div>
  </div>;
}

export default function SettingsPage() { return <Suspense fallback={<div className="settings-page">Loading settings…</div>}><SettingsContent /></Suspense>; }
