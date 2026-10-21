"use client";

import { Button } from '@/components/ui/button';
import { ButtonGroup } from '@/components/ui/button-group';
import { Copy, Save, Loader2 } from 'lucide-react';

interface SummaryUpdaterButtonGroupProps {
  isSaving: boolean;
  isDirty: boolean;
  onSave: () => Promise<void>;
  onCopy: () => Promise<void>;
}

export function SummaryUpdaterButtonGroup({
  isSaving,
  isDirty,
  onSave,
  onCopy,
}: SummaryUpdaterButtonGroupProps) {
  return (
    <ButtonGroup>
      {/* Save button */}
      {(isDirty || isSaving) && <Button
        variant="outline"
        size="sm"
        data-dirty={isDirty ? 'true' : undefined}
        title={isSaving ? "Saving" : "Save changes to the notes"}
        onClick={() => {
          onSave();
        }}
        disabled={isSaving}
      >
        {isSaving ? (
          <>
            <Loader2 className="animate-spin" />
            <span>Saving…</span>
          </>
        ) : (
          <>
            <Save />
            <span>Save</span>
          </>
        )}
      </Button>}

      {/* Copy button */}
      <Button
        variant="outline"
        size="sm"
        title="Copy the notes"
        onClick={() => {
          onCopy();
        }}
        className="cursor-pointer"
      >
        <Copy />
        <span>Copy notes</span>
      </Button>

    </ButtonGroup>
  );
}
