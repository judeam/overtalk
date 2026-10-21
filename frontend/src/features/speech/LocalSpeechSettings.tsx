"use client";

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { toast } from "sonner";
import { CheckCircle2, AlertCircle } from "lucide-react";
import { Switch } from "@/components/ui/switch";
import {
  SettingsSection,
  SettingsRow,
} from "@/components/settings/SettingsSection";
import { LiveSpeakerSettings } from "@/features/speech/LiveSpeakerSettings";

export interface LocalSpeechConfig {
  enabled: boolean;
  python: string;
  diarization_python: string;
  device: string;
  model: string;
  diarization_model: string;
  asr_model_dir: string;
  aligner_model_dir: string;
  diarization_model_path: string;
  live_diarization: boolean;
  streaming_asr_model: string;
  streaming_asr_model_path: string;
}

export function LocalSpeechSettings({
  onModelSelect,
}: {
  onModelSelect?: (model: string) => void;
}) {
  const [config, setConfig] = useState<LocalSpeechConfig | null>(null);
  const [recommended, setRecommended] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [health, setHealth] = useState<unknown>(null);
  useEffect(() => {
    let active = true;
    invoke<{ speech_model: string }>("get_local_quality_profile")
      .then((profile) => { if (active) setRecommended(profile.speech_model); })
      .catch(() => {});
    invoke<LocalSpeechConfig>("speech_get_settings")
      .then((value) => {
        if (active) setConfig(value);
      })
      .catch((reason) => {
        if (active) setError(String(reason));
      });
    return () => {
      active = false;
    };
  }, []);
  if (!config)
    return (
      <p role={error ? "alert" : "status"} className="p-4 text-sm">
        {error ?? "Loading local speech settings…"}
      </p>
    );
  async function save(select: boolean) {
    if (!config) return;
    setBusy(true);
    setError(null);
    try {
      const saved = await invoke<LocalSpeechConfig>("speech_save_settings", {
        settings: config,
      });
      setConfig(saved);
      if (select) {
        const health = await invoke<{ ready: boolean }>("speech_health");
        if (!health.ready)
          throw new Error(
            "Install the selected model and runtime first; use Check installed runtime for details.",
          );
        await invoke("api_save_transcript_config", {
          provider: "localSpeech",
          model: saved.model,
          apiKey: null,
        });
        onModelSelect?.(saved.model);
        window.dispatchEvent(new Event("sonefold-transcription-model-changed"));
      }
      toast.success(
        select ? "Local speech model selected" : "Local speech settings saved",
      );
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  }
  const update = (patch: Partial<LocalSpeechConfig>) =>
    setConfig({ ...config, ...patch });
  return (
    <div>
      <SettingsSection
        title="Local speech"
        description="Models run on this device. Install a runtime and checkpoint before selecting an engine."
      >
        <SettingsRow
          title="Enable local speech"
          description="Use the configured speech runtime."
          htmlFor="enable-local-speech"
        >
          <Switch
            id="enable-local-speech"
            checked={config.enabled}
            onCheckedChange={(enabled) => update({ enabled })}
          />
        </SettingsRow>
        {recommended && <p className="settings-helper">Recommended for this device: {recommended === "qwen3-asr-1.7b" ? "Qwen3-ASR 1.7B" : "Qwen3-ASR 0.6B"}.
          {config.model !== recommended && <Button variant="ghost" size="sm" disabled={busy} onClick={() => update({ model: recommended, asr_model_dir: "" })}>Use recommended model</Button>}
        </p>}
        <div className="settings-form-grid settings-inline-panel">
          <label className="settings-field">
            Speech model
            <select
              value={config.model}
              onChange={(e) => update({ model: e.target.value, asr_model_dir: e.target.value === config.model ? config.asr_model_dir : "" })}
            >
              <option value="qwen3-asr-1.7b">
                Qwen3-ASR 1.7B · higher accuracy, 30 languages
              </option>
              <option value="qwen3-asr-0.6b">
                Qwen3-ASR 0.6B · smaller model
              </option>
              <option value="cohere-transcribe">
                Cohere Transcribe 2B · 14 languages, select language
              </option>
              <option value="moss-transcribe-diarize">
                MOSS 0.9B · joint speech and speaker candidate
              </option>
              <option value="parakeet-tdt-0.6b-v3">
                Parakeet TDT v3 · 25 European languages
              </option>
            </select>
          </label>
          <label className="settings-field">
            Processing device
            <select
              value={config.device}
              onChange={(e) => update({ device: e.target.value })}
            >
              <option value="cuda">NVIDIA CUDA</option>
              <option value="cpu">CPU</option>
              <option value="auto">Automatic</option>
            </select>
          </label>
        </div>
      </SettingsSection>
      <details className="settings-advanced">
        <summary>Runtime and model locations</summary>
        <div className="settings-form-grid">
          <label className="settings-field settings-field-wide">
            Python executable
            <Input
              value={config.python}
              onChange={(e) => update({ python: e.target.value })}
              placeholder="/path/to/speech-env/bin/python"
            />
          </label>
          <label className="settings-field settings-field-wide">
            Speech checkpoint directory
            <Input
              value={config.asr_model_dir}
              onChange={(e) => update({ asr_model_dir: e.target.value })}
              placeholder="Directory, or a .nemo file for Parakeet"
            />
          </label>
          <label className="settings-field settings-field-wide">
            Qwen forced-aligner directory (optional)
            <Input
              value={config.aligner_model_dir}
              onChange={(e) => update({ aligner_model_dir: e.target.value })}
            />
          </label>
        </div>
        <p className="settings-note">
          Leave the speech checkpoint empty to use a complete local Hugging Face
          cache. Models are never downloaded during transcription.
        </p>
      </details>
      <details className="settings-advanced">
        <summary>Speaker analysis</summary>
        <div className="settings-form-grid">
          <label className="settings-field settings-field-wide">
            Speaker model
            <select
              value={config.diarization_model}
              onChange={(e) => update({ diarization_model: e.target.value })}
            >
              <option value="community-1">
                pyannote Community-1 · automatic speaker count
              </option>
              <option value="nemotron-3">
                Nemotron-3 Diarization · up to eight speakers
              </option>
            </select>
          </label>
          <label className="settings-field settings-field-wide">
            Speaker checkpoint
            <Input
              value={config.diarization_model_path}
              onChange={(e) =>
                update({ diarization_model_path: e.target.value })
              }
              placeholder="Community-1 directory or Nemotron .nemo file"
            />
          </label>
          <label className="settings-field settings-field-wide">
            Separate Python executable (optional)
            <Input
              value={config.diarization_python}
              onChange={(e) => update({ diarization_python: e.target.value })}
              placeholder="Use for a separate NeMo environment"
            />
          </label>
        </div>
        <p className="settings-note">
          Community-1 requires accepting its model terms before download. Cohere
          and MOSS use the modern Transformers environment; Qwen and NeMo need
          their own environments. Without a supported aligner, timestamps
          identify audio chunks.
        </p>
        <LiveSpeakerSettings config={config} onChange={update} />
      </details>
      {error && (
        <p role="alert" className="settings-status text-red-700">
          <AlertCircle size={17} />
          {error}
        </p>
      )}
      <div className="settings-footer-actions">
        <Button
          disabled={busy || !config.enabled}
          onClick={() => void save(true)}
        >
          {busy ? "Working…" : "Use for transcription"}
        </Button>
        <Button
          variant="outline"
          disabled={busy}
          onClick={() => void save(false)}
        >
          Save settings
        </Button>
        <Button
          variant="ghost"
          disabled={busy || !config.enabled}
          onClick={async () => {
            setBusy(true);
            setError(null);
            try {
              await invoke("speech_save_settings", { settings: config });
              setHealth(await invoke("speech_health"));
            } catch (reason) {
              setError(String(reason));
            } finally {
              setBusy(false);
            }
          }}
        >
          Check runtime
        </Button>
      </div>
      {health != null && (
        <div>
          <p role="status" className="settings-status">
            {(health as { ready?: boolean }).ready ? (
              <>
                <CheckCircle2 size={17} />
                Runtime dependencies and checkpoint were found.
              </>
            ) : (
              <>
                <AlertCircle size={17} />
                The runtime needs attention. See the details below.
              </>
            )}
          </p>
          <p className="settings-note">
            This checks installation and cached files. Import a short recording
            to verify model loading and device compatibility.
          </p>
          <details className="settings-advanced">
            <summary>Runtime details</summary>
            <pre className="overflow-x-auto rounded-md bg-muted p-3 text-xs">
              {JSON.stringify(health, null, 2)}
            </pre>
          </details>
        </div>
      )}
      <details className="settings-advanced">
        <summary>Installation instructions</summary>
        <p className="settings-note">
          Follow speech-worker/README.md in this repository. Install an
          environment, download your chosen checkpoints once, and enter their
          locations above.
        </p>
      </details>
    </div>
  );
}
