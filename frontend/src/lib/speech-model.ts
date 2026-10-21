/** Shared wire values and presentation used by local speech model managers. */
export type ModelAccuracy = 'High' | 'Good' | 'Decent';

export type ModelStatus =
  | 'Available'
  | 'Missing'
  | { Downloading: { progress: number } }
  | { Error: string }
  | { Corrupted: { file_size: number; expected_min_size: number } };

export type CancelDownloadOutcome = 'cancelled' | 'pending';

export function formatFileSize(sizeMb: number): string {
  if (sizeMb >= 1000) {
    return `${(sizeMb / 1000).toFixed(1)}GB`;
  }
  return `${sizeMb}MB`;
}
