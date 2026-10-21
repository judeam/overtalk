'use client';
import { useRouter } from 'next/navigation';
import { ArrowLeft } from 'lucide-react';

export function BackToWorkspace() {
  const router = useRouter();
  return <button type="button" className="settings-back" onClick={() => {
    const target = new URLSearchParams(window.location.search).get('returnTo');
    router.push(target?.startsWith('/meeting-details?') ? target : '/');
  }}><ArrowLeft size={16} />Back to workspace</button>;
}
