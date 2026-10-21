import { Mic } from 'lucide-react';
import { toast } from 'sonner';
import Analytics from '@/lib/analytics';

/**
 * Shows the recording notification toast with compliance message.
 * Checks user preferences and displays a dismissible toast with:
 * - notice to inform participants
 * - "Don't show again" checkbox
 * - Acknowledgment button
 *
 * @returns Promise<void> - Resolves when notification is shown or skipped
 */
export async function showRecordingNotification(): Promise<void> {
  try {
    const { Store } = await import('@tauri-apps/plugin-store');
    const store = await Store.load('preferences.json');
    const showNotification = await store.get<boolean>('show_recording_notification') ?? true;

    if (showNotification) {
      let dontShowAgain = false;

      const toastId = toast.info('Recording started', {
        icon: <Mic aria-hidden="true" size={18} />,
        style: { background: 'hsl(var(--popover))', color: 'hsl(var(--popover-foreground))', borderColor: 'hsl(var(--border))' },
        description: (
          <div className="space-y-3 min-w-[280px]">
            <p className="text-sm text-foreground">
              Inform all participants this meeting is being recorded.
            </p>
            <label className="flex items-center gap-2 text-xs cursor-pointer hover:bg-muted p-2 rounded transition-colors">
              <input
                type="checkbox"
                onChange={(e) => {
                  dontShowAgain = e.target.checked;
                }}
                className="rounded border-input accent-primary focus:ring-ring focus:ring-2"
              />
              <span className="select-none text-muted-foreground">Don't show this again</span>
            </label>
            <button
              onClick={async () => {
                if (dontShowAgain) {
                  const { Store } = await import('@tauri-apps/plugin-store');
                  const store = await Store.load('preferences.json');
                  await store.set('show_recording_notification', false);
                  await store.save();
                }
                Analytics.trackButtonClick('recording_notification_acknowledged', 'toast');
                toast.dismiss(toastId);
              }}
              className="w-full min-h-10 px-3 py-2 bg-primary text-primary-foreground text-sm rounded-md hover:opacity-90 transition-colors font-medium"
            >
              I've Notified Participants
            </button>
          </div>
        ),
        duration: 10000,
        position: 'bottom-right',
      });
    }
  } catch (notificationError) {
    console.error('Failed to show recording notification:', notificationError);
    // Don't fail the recording if notification fails
  }
}
