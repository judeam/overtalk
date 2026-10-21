'use client';
import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useRouter } from 'next/navigation';
import { toast } from 'sonner';
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/components/ui/dialog';
import type { MeetingSource } from '@/lib/workspace';
import type { InspectorItem } from './Inspector';

export function ReviewActionDialog({ item, meetingId, onClose }: { item: InspectorItem | null; meetingId: string; onClose: () => void }) {
  const [title, setTitle] = useState(''); const [owner, setOwner] = useState(''); const [due, setDue] = useState('');
  const [source, setSource] = useState<MeetingSource | null>(null); const [busy, setBusy] = useState(false); const [error, setError] = useState('');
  const router = useRouter();
  useEffect(() => {
    if (!item) return;
    let active = true; setTitle(item.taskText || item.quote); setOwner(item.person?.label === 'Assignee' ? item.person.name : ''); setDue(''); setSource(null); setError('');
    void invoke<MeetingSource | null>('workspace_source_at', { meetingId, at: item.time }).then(value => { if (active) setSource(value); }).catch(() => {});
    return () => { active = false; };
  }, [item, meetingId]);
  return <Dialog open={!!item} onOpenChange={open => { if (!open && !busy) onClose(); }}><DialogContent className="ot-action-dialog"><DialogTitle>Review action</DialogTitle><DialogDescription>Check the wording and responsibility before adding this to your action list.</DialogDescription>
    <form className="ot-form-stack" onSubmit={async e => {
      e.preventDefault(); setBusy(true); setError('');
      try { await invoke('workspace_save_task', { input: { meeting_id: meetingId, title, owner, due_date: due || null, status: 'open', evidence: source ? [source] : [] } }); onClose(); toast.success('Action added', { action: { label: 'View actions', onClick: () => router.push('/library?tab=actions') } }); }
      catch (e) { setError(String(e)); } finally { setBusy(false); }
    }}>
      <label>What needs to happen?<textarea autoFocus value={title} onChange={e => setTitle(e.target.value)} required maxLength={2000} rows={3} /></label>
      <label>Assignee<input value={owner} onChange={e => setOwner(e.target.value)} placeholder="Leave blank if unassigned" maxLength={200} /></label>
      <label>Due date<input type="date" value={due} onChange={e => setDue(e.target.value)} /></label>
      {source && <blockquote className="ot-task-evidence">“{source.body}”</blockquote>}
      {error && <p role="alert" className="ot-danger-text">{error}</p>}
      <div className="ot-inline-form"><button className="ot-button ot-button-primary" disabled={busy || !title.trim()}>{busy ? 'Saving…' : 'Add action'}</button><button type="button" className="ot-button ot-button-secondary" disabled={busy} onClick={onClose}>Cancel</button></div>
    </form>
  </DialogContent></Dialog>;
}
