import type { ModelAccuracy, ModelStatus, CancelDownloadOutcome } from './speech-model';
export { formatFileSize } from './speech-model';
export type { ModelStatus, CancelDownloadOutcome } from './speech-model';

// Types for Parakeet (NVIDIA NeMo) integration
export interface ParakeetModelInfo {
  name: string;
  path: string;
  size_mb: number;
  accuracy: ModelAccuracy;
  speed: ProcessingSpeed;
  status: ModelStatus;
  description?: string;
  quantization: QuantizationType;
}

type QuantizationType = 'FP32' | 'Int8';
type ProcessingSpeed = 'Slow' | 'Medium' | 'Fast' | 'Very Fast' | 'Ultra Fast';

type ParakeetDownloadEventStatus = 'downloading' | 'completed' | 'cancelled';

export interface ParakeetDownloadProgressEvent {
  modelName: string;
  progress: number;
  downloaded_bytes?: number;
  total_bytes?: number;
  downloaded_mb?: number;
  total_mb?: number;
  speed_mbps?: number;
  status: ParakeetDownloadEventStatus;
}

// User-friendly model display configuration
export interface ModelDisplayInfo {
  friendlyName: string;
  icon: string;
  tagline: string;
  recommended?: boolean;
  tier: 'fastest' | 'balanced' | 'precise';
}

const MODEL_DISPLAY_CONFIG: Record<string, ModelDisplayInfo> = {
  'parakeet-tdt-0.6b-v3-int8': {
    friendlyName: 'Lightning',
    icon: '⚡',
    tagline: '25 European languages • CPU INT8',
    recommended: true,
    tier: 'fastest'
  },
  'parakeet-tdt-0.6b-v2-int8': {
    friendlyName: 'Compact',
    icon: '📦',
    tagline: 'English only • Smaller size',
    tier: 'balanced'
  }
};

// Model configuration for Parakeet models (matching Rust implementation)
// Supported models: parakeet-tdt-0.6b in v2 and v3 variants
// Source: https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx

// Get user-friendly display name for a model
export function getModelDisplayName(modelName: string): string {
  const displayInfo = MODEL_DISPLAY_CONFIG[modelName];
  return displayInfo?.friendlyName || modelName;
}

// Get model display info (icon, tagline, etc.)
export function getModelDisplayInfo(modelName: string): ModelDisplayInfo | null {
  return MODEL_DISPLAY_CONFIG[modelName] || null;
}

// Tauri command wrappers for Parakeet backend
import { invoke } from '@tauri-apps/api/core';

export class ParakeetAPI {
  static async init(): Promise<void> {
    await invoke('parakeet_init');
  }

  static async getAvailableModels(): Promise<ParakeetModelInfo[]> {
    return await invoke('parakeet_get_available_models');
  }

  static async downloadModel(modelName: string): Promise<void> {
    await invoke('parakeet_download_model', { modelName });
  }

  static async cancelDownload(modelName: string): Promise<CancelDownloadOutcome> {
    return await invoke('parakeet_cancel_download', { modelName });
  }

  static async deleteCorruptedModel(modelName: string): Promise<string> {
    return await invoke('parakeet_delete_corrupted_model', { modelName });
  }
}
