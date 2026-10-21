"use client";

import { MeetingSummary, Summary, Transcript } from "@/types";
import {
  BlockNoteSummaryView,
  BlockNoteSummaryViewRef,
} from "@/components/AISummary/BlockNoteSummaryView";
import { EmptyStateSummary } from "@/components/EmptyStateSummary";
import { ModelConfig } from "@/components/ModelSettingsModal";
import { SummaryGeneratorButtonGroup } from "./SummaryGeneratorButtonGroup";
import { SummaryUpdaterButtonGroup } from "./SummaryUpdaterButtonGroup";
import { useEffect, useRef, useState, RefObject } from "react";
import { toast } from "sonner";
import { Languages, ChevronDown } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Popover,
  PopoverTrigger,
  PopoverContent,
} from "@/components/ui/popover";
import { LanguagePickerPopover } from "@/components/LanguagePickerPopover";
import { useRecentLanguages } from "@/hooks/useRecentLanguages";
import { labelForCode } from "@/lib/summary-languages";
import {
  readMeetingSummaryLanguage,
  saveMeetingSummaryLanguage,
  SummaryLanguageStorage,
} from "@/lib/summary-language-preferences";
import { hasVisibleSummaryContent } from "@/lib/summary-content";
import { SummaryTemplateEditor } from "./SummaryTemplateEditor";

interface SummaryPanelProps {
  sourceTimes?: ([number, number] | null)[];
  onSource?: (seconds: number) => void;
  meeting: {
    id: string;
    title: string;
    created_at: string;
  };
  meetingTitle: string;
  isSummaryDirty: boolean;
  summaryRef: RefObject<BlockNoteSummaryViewRef | null>;
  isSaving: boolean;
  onSaveAll: () => Promise<void>;
  onCopySummary: () => Promise<void>;
  aiSummary: MeetingSummary | null;
  summaryStatus:
    | "idle"
    | "processing"
    | "summarizing"
    | "regenerating"
    | "completed"
    | "error";
  transcripts: Transcript[];
  modelConfig: ModelConfig;
  setModelConfig: (
    config: ModelConfig | ((prev: ModelConfig) => ModelConfig),
  ) => void;
  onSaveModelConfig: (config?: ModelConfig) => Promise<void>;
  onGenerateSummary: (customPrompt: string) => Promise<void>;
  onStopGeneration: () => void;
  customPrompt: string;
  onSaveSummary: (summary: MeetingSummary) => Promise<void>;
  onSummaryChange: (summary: Summary) => void;
  onDirtyChange: (isDirty: boolean) => void;
  summaryError: string | null;
  onRegenerateSummary: () => Promise<void>;
  getSummaryStatusMessage: (
    status:
      | "idle"
      | "processing"
      | "summarizing"
      | "regenerating"
      | "completed"
      | "error",
  ) => string;
  availableTemplates: Array<{ id: string; name: string; description: string }>;
  selectedTemplate: string;
  onTemplateSelect: (templateId: string, templateName: string) => void;
  isModelConfigLoading?: boolean;
  onOpenModelSettings?: (openFn: () => void) => void;
  /** Extra tools at the end of the notes toolbar (export, close). */
  headerExtras?: React.ReactNode;
}

export function SummaryPanel({
  meeting,
  meetingTitle,
  isSummaryDirty,
  summaryRef,
  isSaving,
  onSaveAll,
  onCopySummary,
  aiSummary,
  summaryStatus,
  transcripts,
  modelConfig,
  setModelConfig,
  onSaveModelConfig,
  onGenerateSummary,
  onStopGeneration,
  customPrompt,
  onSaveSummary,
  onSummaryChange,
  onDirtyChange,
  summaryError,
  onRegenerateSummary,
  getSummaryStatusMessage,
  availableTemplates,
  selectedTemplate,
  onTemplateSelect,
  isModelConfigLoading = false,
  onOpenModelSettings,
  headerExtras,
  sourceTimes,
  onSource,
}: SummaryPanelProps) {
  const [summaryLang, setSummaryLang] = useState<string | null>(null);
  const [summaryLangStorage, setSummaryLangStorage] =
    useState<SummaryLanguageStorage>("metadata");
  const [langPickerOpen, setLangPickerOpen] = useState(false);
  const languageLoadVersionRef = useRef(0);
  const activeMeetingIdRef = useRef(meeting.id);
  const languageSaveVersionRef = useRef(0);
  const languageSaveLoopRunningRef = useRef(false);
  const latestLanguageSaveRequestRef = useRef<{
    version: number;
    meetingId: string;
    language: string | null;
    rollback: {
      language: string | null;
      storage: SummaryLanguageStorage;
    };
  } | null>(null);
  activeMeetingIdRef.current = meeting.id;
  const { addRecent } = useRecentLanguages();

  const effectiveLangLabel = summaryLang ? labelForCode(summaryLang) : "Auto";
  const isLocalFallbackLanguage = summaryLangStorage === "local_fallback";
  const autoSubtitle = isLocalFallbackLanguage
    ? "Saved on this device for folderless meetings"
    : "Uses dominant transcript language";

  useEffect(() => {
    let cancelled = false;
    const loadVersion = languageLoadVersionRef.current + 1;
    languageLoadVersionRef.current = loadVersion;

    const loadSummaryLanguage = async () => {
      try {
        const stored = await readMeetingSummaryLanguage(meeting.id);
        if (!cancelled && languageLoadVersionRef.current === loadVersion) {
          setSummaryLang(stored.language);
          setSummaryLangStorage(stored.storage);
        }
      } catch (err) {
        console.error("Failed to load summary language:", err);
        toast.warning("Could not load saved summary language", {
          description: "Using Auto until meeting metadata can be read.",
        });
        if (!cancelled && languageLoadVersionRef.current === loadVersion)
          setSummaryLang(null);
      }
    };

    loadSummaryLanguage();

    return () => {
      cancelled = true;
    };
  }, [meeting.id]);

  const persistLatestLanguageSelection = async () => {
    if (languageSaveLoopRunningRef.current) return;
    languageSaveLoopRunningRef.current = true;

    try {
      while (true) {
        const request = latestLanguageSaveRequestRef.current;
        if (!request) return;

        try {
          const saved = await saveMeetingSummaryLanguage(
            request.meetingId,
            request.language,
          );
          const latest = latestLanguageSaveRequestRef.current;
          if (
            latest?.version === request.version &&
            activeMeetingIdRef.current === request.meetingId
          ) {
            setSummaryLang(saved.language);
            setSummaryLangStorage(saved.storage);
            if (saved.storage === "local_fallback") {
              toast.info("Summary language saved on this device", {
                description:
                  "This meeting has no recording folder, so the preference cannot be written to meeting metadata.",
              });
            }
            if (request.language) {
              addRecent(request.language);
            }
            return;
          }

          if (latest?.version === request.version) return;
        } catch (err) {
          const latest = latestLanguageSaveRequestRef.current;
          if (
            latest?.version === request.version &&
            activeMeetingIdRef.current === request.meetingId
          ) {
            console.error("Failed to persist summary language:", err);
            toast.error("Failed to save summary language");
            setSummaryLang(request.rollback.language);
            setSummaryLangStorage(request.rollback.storage);
            return;
          }

          console.warn("Ignoring failed stale summary language save:", err);
          if (latest?.version === request.version) return;
        }
      }
    } finally {
      languageSaveLoopRunningRef.current = false;
    }
  };

  const handleLangChange = (code: string | null) => {
    const previous = summaryLang;
    const previousStorage = summaryLangStorage;
    const nextStored = code;
    languageLoadVersionRef.current += 1;
    latestLanguageSaveRequestRef.current = {
      version: languageSaveVersionRef.current + 1,
      meetingId: meeting.id,
      language: nextStored,
      rollback: {
        language: previous,
        storage: previousStorage,
      },
    };
    languageSaveVersionRef.current += 1;
    setSummaryLang(nextStored);
    setLangPickerOpen(false);
    void persistLatestLanguageSelection();
  };

  const isSummaryLoading =
    summaryStatus === "processing" ||
    summaryStatus === "summarizing" ||
    summaryStatus === "regenerating";
  const hasSummary = Boolean(aiSummary && Array.isArray((aiSummary as { summary_json?: unknown }).summary_json)) || hasVisibleSummaryContent(aiSummary);

  const languageSlot = (
    <Popover open={langPickerOpen} onOpenChange={setLangPickerOpen}>
      <PopoverTrigger asChild>
        <Button
          variant="outline"
          size="sm"
          title={`Summary language: ${effectiveLangLabel}${isLocalFallbackLanguage ? " (saved on this device)" : ""}`}
          aria-label="Set summary language"
        >
          <Languages size={18} />
          <span className="hidden @[40rem]:inline">{effectiveLangLabel}</span>
          <ChevronDown size={14} className="text-gray-400" />
        </Button>
      </PopoverTrigger>
      <PopoverContent
        align="end"
        className="w-auto p-0 border-0 shadow-none bg-transparent"
      >
        <LanguagePickerPopover
          value={summaryLang}
          onChange={handleLangChange}
          onClose={() => setLangPickerOpen(false)}
          autoSubtitle={autoSubtitle}
        />
      </PopoverContent>
    </Popover>
  );

  return (
    <div className="meeting-notes-panel flex-1 min-w-0 flex flex-col bg-card overflow-hidden h-full w-full @container">
      <div className="meeting-pane-header ot-notes-toolbar">
        <div className="ot-notes-heading">
          <h3>Notes</h3>
          <span>{isSummaryDirty ? "Saving your changes…" : hasSummary ? "You can edit these notes" : "No notes yet"}</span>
        </div>
        <div className="ot-notes-actions" data-tour="generate">
          <SummaryGeneratorButtonGroup
            modelConfig={modelConfig}
            setModelConfig={setModelConfig}
            onSaveModelConfig={onSaveModelConfig}
            onGenerateSummary={onGenerateSummary}
            onStopGeneration={onStopGeneration}
            customPrompt={customPrompt}
            summaryStatus={summaryStatus}
            availableTemplates={availableTemplates}
            selectedTemplate={selectedTemplate}
            onTemplateSelect={onTemplateSelect}
            hasTranscripts={transcripts.length > 0}
            hasSummary={hasSummary}
            isModelConfigLoading={isModelConfigLoading}
            onOpenModelSettings={onOpenModelSettings}
            languageSlot={transcripts.length > 0 || hasSummary ? languageSlot : undefined}
          />
          <SummaryTemplateEditor
            templates={availableTemplates}
            selectedTemplate={selectedTemplate}
            onSelect={onTemplateSelect}
            disabled={isSummaryLoading}
          />
          {hasSummary && !isSummaryLoading && (
            <SummaryUpdaterButtonGroup isSaving={isSaving} isDirty={isSummaryDirty} onSave={onSaveAll} onCopy={onCopySummary} />
          )}
          {headerExtras}
        </div>
      </div>

      {isSummaryLoading ? (
        <div className="flex items-center justify-center flex-1">
          <div className="text-center">
            <div
              className="inline-block animate-spin rounded-full h-8 w-8 border-2 border-muted border-t-primary mb-4"
              role="status"
              aria-label="Generating summary"
            ></div>
            <p className="text-muted-foreground">Writing your notes…</p>
          </div>
        </div>
      ) : !hasSummary ? (
        <div className="ot-notes-empty"><button type="button" className="ot-button ot-button-secondary" onClick={async () => {
          const notes = { markdown: '# Notes\n\n' };
          try { await onSaveSummary(notes); onSummaryChange(notes as unknown as Summary); }
          catch (e) { toast.error('Could not create notes', { description: String(e) }); }
        }}>Write notes yourself</button>
        <EmptyStateSummary
          onGenerate={() => onGenerateSummary(customPrompt)}
          hasModel={modelConfig.provider !== null && modelConfig.model !== null}
          isGenerating={isSummaryLoading}
          error={summaryError}
        /></div>
      ) : (
        <div className="flex-1 overflow-y-auto overflow-x-auto min-h-0">
          <div className="meeting-notes-document">
            <BlockNoteSummaryView
              sourceTimes={sourceTimes}
              onSource={onSource}
              ref={summaryRef}
              summaryData={aiSummary}
              onSave={onSaveSummary}
              onSummaryChange={onSummaryChange}
              onDirtyChange={onDirtyChange}
              status={summaryStatus}
              error={summaryError}
              onRegenerateSummary={() => {
                onRegenerateSummary();
              }}
              meeting={{
                id: meeting.id,
                title: meetingTitle,
                created_at: meeting.created_at,
              }}
            />
          </div>
          {(summaryStatus === "error" || summaryStatus === "completed") && (
            <p
              className={`meeting-summary-status ${summaryStatus === "error" ? "text-destructive" : "text-muted-foreground"}`}
              role="status"
            >
              {getSummaryStatusMessage(summaryStatus)}
            </p>
          )}
        </div>
      )}
    </div>
  );
}
