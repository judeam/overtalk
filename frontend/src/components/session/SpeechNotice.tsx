'use client';

import { useState, type ReactNode } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Cpu, Loader2 } from 'lucide-react';

export interface SpeechNoticeData {
  reason: 'gpu-memory' | 'engine' | string;
  message: string;
  detail: string;
  can_free: boolean;
}

/** Explains why speech recognition could not run, and frees GPU memory held by Ollama when that is the cause. */
export function SpeechNotice({ title, notice, after, children, onFreed }: {
  title: string;
  notice: SpeechNoticeData;
  after?: string;
  children?: ReactNode;
  onFreed?: () => void;
}) {
  const [freeing, setFreeing] = useState(false);
  const [freed, setFreed] = useState<string[] | null>(null);
  const free = async () => {
    setFreeing(true);
    try {
      const names = await invoke<string[]>('gpu_free_ollama');
      setFreed(names);
      toast.success(names.length ? `Freed GPU memory held by ${names.join(', ')}` : 'Nothing was holding GPU memory in Ollama');
      onFreed?.();
    } catch (error) {
      toast.error('Could not free GPU memory', { description: String(error) });
    } finally {
      setFreeing(false);
    }
  };
  return (
    <div className="ot-notice" role="status">
      <strong>{title}</strong>
      <p>{notice.message}{after ? ` ${after}` : ''}</p>
      {notice.reason !== 'gpu-memory' && notice.detail && <p className="ot-notice-detail">{notice.detail}</p>}
      {(notice.can_free || children) && (
        <div className="ot-notice-actions">
          {notice.can_free && !freed && (
            <button type="button" className="ot-button ot-button-secondary ot-button-small" onClick={() => void free()} disabled={freeing}>
              {freeing ? <Loader2 className="ot-spin" aria-hidden="true" /> : <Cpu aria-hidden="true" />}Free GPU memory
            </button>
          )}
          {children}
        </div>
      )}
      {freed && freed.length > 0 && <p className="ot-notice-detail">Ollama loads {freed.length === 1 ? 'that model' : 'those models'} again the next time an app asks for {freed.length === 1 ? 'it' : 'them'}.</p>}
    </div>
  );
}
