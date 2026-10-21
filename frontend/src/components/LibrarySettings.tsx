"use client";
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { Switch } from "./ui/switch";
import {
  SettingsSection,
  SettingsRow,
  SettingsLoading,
} from "./settings/SettingsSection";
import { toast } from "sonner";
interface Config {
  enabled: boolean;
  python: string;
  device: string;
  family: string;
  size: string;
  reranker_size: string;
  embedding_dir: string;
  reranker_dir: string;
  rerank: boolean;
}
interface Health {
  enabled: boolean;
  total: number;
  indexed: number;
  size: string;
  runtime: {
    embedding: { cached: boolean; revision?: string; diagnostic?: string };
    reranker: { cached: boolean; revision?: string; diagnostic?: string };
  };
}
interface Restore {
  directory: string;
  database: string;
  audio_included: boolean;
  credentials_restored: boolean;
}
export function LibrarySettings() {
  const [config, setConfig] = useState<Config | null>(null);
  const [busy, setBusy] = useState("");
  const [error, setError] = useState("");
  const [backupError, setBackupError] = useState("");
  const [health, setHealth] = useState<Health | null>(null);
  const [progress, setProgress] = useState<{
    inspected: number;
    updated: number;
  } | null>(null);
  const [password, setPassword] = useState("");
  const [confirmation, setConfirmation] = useState("");
  const [includeAudio, setIncludeAudio] = useState(true);
  const [restore, setRestore] = useState<Restore | null>(null);
  const [diagnostics, setDiagnostics] = useState<{
    checks: { name: string; status: string; detail: string }[];
  } | null>(null);
  useEffect(() => {
    let active = true;
    invoke<Config>("library_get_settings")
      .then((c) => {
        if (active) setConfig(c);
      })
      .catch((e) => {
        if (active) setError(String(e));
      });
    const listener = listen<{ inspected: number; updated: number }>(
      "library-index-progress",
      (e) => {
        if (active) setProgress(e.payload);
      },
    );
    return () => {
      active = false;
      void listener.then((remove) => remove());
    };
  }, []);
  const update = (patch: Partial<Config>) => {
    if (config) setConfig({ ...config, ...patch });
    setHealth(null);
  };
  const save = async () => {
    if (!config) return;
    setBusy("save");
    setError("");
    try {
      await invoke("library_save_settings", { settings: config });
      toast.success("Library settings saved");
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy("");
    }
  };
  const check = async () => {
    setBusy("check");
    setError("");
    try {
      if (config) await invoke("library_save_settings", { settings: config });
      setHealth(await invoke<Health>("library_health"));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy("");
    }
  };
  const index = async () => {
    setBusy("index");
    setError("");
    setProgress(null);
    try {
      await invoke("library_index");
      setHealth(await invoke<Health>("library_health"));
      toast.success("Semantic index is up to date");
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy("");
    }
  };
  const backup = async () => {
    if (password.length < 12 || password !== confirmation) {
      setBackupError("Enter matching passphrases of at least 12 characters.");
      return;
    }
    setBusy("backup");
    setBackupError("");
    try {
      const path = await invoke<string | null>("workspace_pick_file", {
        kind: "backup-save",
      });
      if (!path) return;
      await invoke("workspace_create_backup", {
        path,
        passphrase: password,
        includeAudio,
      });
      setPassword("");
      setConfirmation("");
      toast.success("Encrypted backup saved");
    } catch (e) {
      setBackupError(String(e));
    } finally {
      setBusy("");
    }
  };
  const restoreCopy = async () => {
    if (!password) {
      setBackupError("Enter the backup passphrase first.");
      return;
    }
    setBusy("restore");
    setBackupError("");
    setRestore(null);
    try {
      const path = await invoke<string | null>("workspace_pick_file", {
        kind: "backup-open",
      });
      if (!path) return;
      const directory = await invoke<string | null>("workspace_pick_file", {
        kind: "restore-folder",
      });
      if (!directory) return;
      setRestore(
        await invoke<Restore>("workspace_restore_backup", {
          path,
          directory,
          passphrase: password,
        }),
      );
      setPassword("");
      setConfirmation("");
      toast.success("Backup restored to a separate folder");
    } catch (e) {
      setBackupError(String(e));
    } finally {
      setBusy("");
    }
  };
  if (!config) return error ? <p role="alert">{error}</p> : <SettingsLoading />;
  return (
    <div>
      <SettingsSection
        title="Search that understands the conversation"
        description="Combine exact terms with local semantic retrieval, then rank the evidence before answering."
      >
        <SettingsRow
          title="Semantic search"
          description="Find related ideas even when the wording differs. Exact-word search works without these models. Advanced search requires separately installed models; choose a profile for your computer below."
          htmlFor="library-enabled"
        >
          <Switch
            id="library-enabled"
            checked={config.enabled}
            disabled={!!busy}
            onCheckedChange={(enabled) => update({ enabled })}
          />
        </SettingsRow>
        <SettingsRow
          title="Improve the order of search results"
          description="Use a second local model to put the most relevant passages first. This uses more processing time."
          htmlFor="library-rerank"
        >
          <Switch
            id="library-rerank"
            checked={config.rerank}
            disabled={!!busy}
            onCheckedChange={(rerank) => update({ rerank })}
          />
        </SettingsRow>
        <details className="settings-advanced">
          <summary>Runtime &amp; quality profile</summary>
          <div className="settings-form-grid">
            <label className="settings-field settings-field-wide">
              Embedding profile
              <select
                value={config.family === "nemotron3" ? "quality" : config.size}
                disabled={!!busy}
                onChange={(e) =>
                  update(
                    e.target.value === "quality"
                      ? { family: "nemotron3", size: "8B" }
                      : { family: "qwen3", size: e.target.value },
                  )
                }
              >
                <option value="quality">Quality · Nemotron 3 Embed 8B</option>
                <option value="8B">Apache profile · Qwen3 Embed 8B</option>
                <option value="4B">Balanced · Qwen3 Embed 4B</option>
                <option value="0.6B">Compact · Qwen3 Embed 0.6B</option>
              </select>
            </label>
            <label className="settings-field">
              Reranker size
              <select
                value={config.reranker_size}
                disabled={!!busy}
                onChange={(e) => update({ reranker_size: e.target.value })}
              >
                <option value="4B">Qwen3 4B · quality</option>
                <option value="8B">Qwen3 8B</option>
                <option value="0.6B">Qwen3 0.6B · compact</option>
              </select>
            </label>
            <label className="settings-field">
              Device
              <select
                value={config.device}
                disabled={!!busy}
                onChange={(e) => update({ device: e.target.value })}
              >
                <option value="auto">Automatic</option>
                <option value="cuda">NVIDIA CUDA</option>
                <option value="cpu">CPU</option>
              </select>
            </label>
            <label className="settings-field settings-field-wide">
              Python executable
              <Input
                value={config.python}
                disabled={!!busy}
                onChange={(e) => update({ python: e.target.value })}
                placeholder="/path/to/modern-env/bin/python"
              />
            </label>
            <label className="settings-field settings-field-wide">
              Embedding directory
              <Input
                value={config.embedding_dir}
                disabled={!!busy}
                onChange={(e) => update({ embedding_dir: e.target.value })}
                placeholder="Pinned local weights, or leave empty for local cache"
              />
            </label>
            <label className="settings-field settings-field-wide">
              Reranker directory
              <Input
                value={config.reranker_dir}
                disabled={!!busy}
                onChange={(e) => update({ reranker_dir: e.target.value })}
              />
            </label>
          </div>
          <p className="settings-note">
            Use the model download utility to record the upstream revision.
            Inference is offline. Nemotron uses OpenMDW 1.1; Qwen models use
            Apache 2.0. Rebuild the index after changing profiles. Search embeds
            up to 6,000 characters per passage; exact search covers the full
            text.
          </p>
        </details>
        <div className="settings-footer-actions">
          <Button disabled={!!busy} onClick={() => void save()}>
            {busy === "save" ? "Saving…" : "Save settings"}
          </Button>
          <Button
            variant="outline"
            disabled={!!busy || !config.enabled}
            onClick={() => void check()}
          >
            Check models
          </Button>
          <Button
            variant="outline"
            disabled={!!busy || !config.enabled}
            onClick={() => void index()}
          >
            {busy === "index" ? "Indexing…" : "Build semantic index"}
          </Button>
          {busy === "index" && (
            <Button
              variant="ghost"
              onClick={() => void invoke("library_cancel_index")}
            >
              Stop indexing
            </Button>
          )}
        </div>
        {progress && (
          <p role="status" className="settings-note">
            Checked {progress.inspected} passages; updated {progress.updated}.
            Completed batches remain usable if you stop.
          </p>
        )}
        {health && (
          <div className="settings-inline-panel">
            <p>
              {health.indexed} indexed passages of {health.total} ·{" "}
              {health.runtime.embedding.cached
                ? "Embedding checkpoint found"
                : "Embedding checkpoint missing"}{" "}
              ·{" "}
              {health.runtime.reranker.cached
                ? "Reranker found"
                : "Reranker missing"}
            </p>
            {health.runtime.embedding.diagnostic && (
              <p className="settings-note">
                {health.runtime.embedding.diagnostic}
              </p>
            )}
            {health.runtime.reranker.diagnostic && (
              <p className="settings-note">
                {health.runtime.reranker.diagnostic}
              </p>
            )}
            <p className="settings-note">
              File readiness checks do not validate model loading. Build the
              index to test the runtime.
            </p>
          </div>
        )}
      </SettingsSection>
      <SettingsSection
        title="Encrypted backups"
        description="Save your meetings, notes, actions and optional audio in an authenticated age backup. Provider credentials and model weights are excluded."
      >
        <SettingsRow
          title="Include recordings"
          description="Keep the audio alongside the transcripts in your backup."
          htmlFor="backup-audio"
        >
          <Switch
            id="backup-audio"
            checked={includeAudio}
            disabled={!!busy}
            onCheckedChange={setIncludeAudio}
          />
        </SettingsRow>
        <div className="settings-form-grid settings-inline-panel">
          <label className="settings-field">
            Backup passphrase
            <Input
              type="password"
              autoComplete="new-password"
              aria-label="Backup passphrase"
              aria-describedby="backup-passphrase-help backup-validation"
              aria-invalid={!!backupError}
              value={password}
              disabled={!!busy}
              onChange={(e) => {
                setPassword(e.target.value);
                setBackupError("");
              }}
            />
            <span id="backup-passphrase-help" className="settings-note">
              At least 12 characters for a new backup. For restore, use its
              existing passphrase.
            </span>
          </label>
          <label className="settings-field">
            Confirm for a new backup
            <Input
              type="password"
              autoComplete="new-password"
              aria-label="Confirm for a new backup"
              aria-describedby="backup-confirmation-help backup-validation"
              aria-invalid={!!backupError}
              value={confirmation}
              disabled={!!busy}
              onChange={(e) => {
                setConfirmation(e.target.value);
                setBackupError("");
              }}
            />
            <span id="backup-confirmation-help" className="settings-note">
              Match the passphrase when saving a new backup. Not required for
              restore.
            </span>
          </label>
        </div>
        <div id="backup-validation" role={backupError ? "alert" : undefined}>
          {backupError && <p className="library-error">{backupError}</p>}
        </div>
        <p className="settings-note">
          Keep this passphrase somewhere safe; it is never saved by Overtalk.
          Restore creates a separate folder with an integrity-checked database
          and audio. Your current workspace stays in place. A restore copy is
          not multi-device sync.
        </p>
        <div className="settings-footer-actions">
          <Button disabled={!!busy || !password} onClick={() => void backup()}>
            {busy === "backup" ? "Encrypting backup…" : "Save encrypted backup"}
          </Button>
          <Button
            variant="outline"
            disabled={!!busy || !password}
            onClick={() => void restoreCopy()}
          >
            {busy === "restore" ? "Restoring copy…" : "Restore a copy"}
          </Button>
        </div>
        {restore && (
          <div className="settings-inline-panel" role="status">
            <p>Restored copy</p>
            <p className="break-all text-sm">{restore.directory}</p>
            <p className="settings-note">
              The database and recording paths are ready in this folder.
              Credentials must be configured again. See the backup guide before
              replacing an existing profile.
            </p>
          </div>
        )}
      </SettingsSection>
      <SettingsSection
        title="Runtime diagnostics"
        description="Check the database, runtime files and storage permissions without sending your data anywhere."
      >
        <Button
          variant="outline"
          disabled={!!busy}
          onClick={async () => {
            setBusy("diagnostics");
            setError("");
            try {
              setDiagnostics(await invoke("workspace_diagnostics"));
            } catch (e) {
              setError(String(e));
            } finally {
              setBusy("");
            }
          }}
        >
          {busy === "diagnostics" ? "Checking…" : "Run diagnostics"}
        </Button>
        {diagnostics &&
          diagnostics.checks.map((item, i) => (
            <div className="settings-row" key={i}>
              <div className="settings-row-copy">
                <p className="settings-row-title">{item.name}</p>
                <p>{item.detail}</p>
              </div>
              <span className="text-sm text-muted-foreground">
                {item.status}
              </span>
            </div>
          ))}
      </SettingsSection>
      {error && (
        <p className="library-error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
