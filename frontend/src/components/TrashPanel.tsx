'use client';
import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Button } from './ui/button';
import { ConfirmationModal } from './ConfirmationModel/confirmation-modal';
import type { WorkspaceTask } from '@/lib/workspace';

type TrashedMeeting = { meeting_id: string; title: string; trashed_at: string | null };
export function TrashPanel() {
  const [meetings, setMeetings] = useState<TrashedMeeting[]>([]);
  const [tasks, setTasks] = useState<WorkspaceTask[]>([]);
  const [pending, setPending] = useState<TrashedMeeting | null>(null);
  const [busy, setBusy] = useState(false); const [error, setError] = useState('');
  const load = async () => {
    const [rows, actions] = await Promise.all([invoke<TrashedMeeting[]>('workspace_meeting_states'), invoke<WorkspaceTask[]>('workspace_tasks')]);
    setMeetings(rows.filter(m => m.trashed_at)); setTasks(actions.filter(t => t.status === 'cancelled'));
  };
  useEffect(() => { void load().catch(e => setError(String(e))); }, []);
  const run = async (work: () => Promise<unknown>) => { setBusy(true); setError(''); try { await work(); await load(); } catch(e) { setError(String(e)); } finally { setBusy(false); } };
  return <section className="library-trash">
    <div className="library-section-heading"><div><h2>Trash</h2><p>Restore a meeting or action whenever you need it.</p></div></div>
    {error && <p role="alert" className="library-error">{error}</p>}
    {!meetings.length && !tasks.length && <div className="library-empty"><h3>Trash is empty</h3><p>Meetings and actions you remove will be kept here until you delete them permanently.</p></div>}
    {meetings.map(meeting => <article className="library-trash-row" key={meeting.meeting_id}><div><strong>{meeting.title}</strong><p>Meeting · {new Date(meeting.trashed_at!).toLocaleDateString()}</p></div><Button variant="outline" disabled={busy} onClick={() => void run(() => invoke('workspace_organise_meeting', { change: { meeting_id: meeting.meeting_id, trashed: false } }))}>Restore</Button><Button variant="ghost" disabled={busy} onClick={() => setPending(meeting)}>Delete permanently</Button></article>)}
    {tasks.map(task => <article className="library-trash-row" key={task.id}><div><strong>{task.title}</strong><p>Action</p></div><Button variant="outline" disabled={busy} onClick={() => void run(() => invoke('workspace_task_status', { id: task.id, status: 'open' }))}>Restore</Button></article>)}
    <ConfirmationModal isOpen={!!pending} text={`Permanently delete “${pending?.title ?? ''}” and its saved transcript, notes and actions? This cannot be undone.`} onCancel={() => setPending(null)} onConfirm={() => { const id = pending?.meeting_id; setPending(null); if (id) void run(async () => { await invoke('api_delete_meeting', { meetingId: id }); toast.success('Meeting permanently deleted'); }); }} />
  </section>;
}
