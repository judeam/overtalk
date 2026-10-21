'use client';

import { useState } from 'react';
import { Download, Loader2 } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuSub, DropdownMenuSubContent, DropdownMenuSubTrigger, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { createMeetingExport, downloadMeetingExport, MeetingExportContent, MeetingExportData, MeetingExportFormat } from '@/lib/meeting-export';

export interface ExportMeetingMenuProps { onGetData: () => Promise<MeetingExportData>; disabled?: boolean }
const formats: [MeetingExportFormat, string][] = [['md', 'Markdown (.md)'], ['docx', 'Word (.docx)'], ['pdf', 'PDF (.pdf)'], ['txt', 'Plain text (.txt)'], ['json', 'JSON (.json)']];

export function ExportMeetingMenu({ onGetData, disabled }: ExportMeetingMenuProps) {
  const [busy, setBusy] = useState(false);
  const exportMeeting = async (format: MeetingExportFormat, content: MeetingExportContent) => {
    setBusy(true);
    try {
      const result = await createMeetingExport(await onGetData(), format, content);
      downloadMeetingExport(result); toast.success(`Exported ${result.filename}`);
    } catch (error) { toast.error('Could not export meeting', { description: String(error) }); } finally { setBusy(false); }
  };
  return <DropdownMenu>
    <DropdownMenuTrigger asChild><Button variant="outline" size="sm" disabled={busy || disabled}>{busy ? <Loader2 size={16} className="animate-spin" /> : <Download size={16} />}Export</Button></DropdownMenuTrigger>
    <DropdownMenuContent align="end">
      <DropdownMenuLabel>Export meeting</DropdownMenuLabel><DropdownMenuSeparator />
      {([['summary', 'Summary'], ['transcript', 'Transcript'], ['both', 'Summary and transcript']] as [MeetingExportContent, string][]).map(([content, label]) => <DropdownMenuSub key={content}><DropdownMenuSubTrigger>{label}</DropdownMenuSubTrigger><DropdownMenuSubContent>{formats.map(([format, title]) => <DropdownMenuItem key={format} onSelect={() => void exportMeeting(format, content)}>{title}</DropdownMenuItem>)}</DropdownMenuSubContent></DropdownMenuSub>)}
    </DropdownMenuContent>
  </DropdownMenu>;
}
