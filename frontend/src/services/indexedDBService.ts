/** Browser recovery checkpoints. A successful write means its whole transaction committed. */
import type { TranscriptUpdate } from '@/types';

export interface MeetingMetadata {
  meetingId: string;
  title: string;
  startTime: number;
  lastUpdated: number;
  transcriptCount: number;
  savedToSQLite: boolean;
  folderPath?: string;
}

type RecoveryTranscript = Pick<TranscriptUpdate, 'text' | 'timestamp'> &
  Partial<TranscriptUpdate> & { id?: string | number; sequenceId?: number };

export interface StoredTranscript extends Partial<TranscriptUpdate> {
  id?: string | number;
  meetingId: string;
  text: string;
  timestamp: string;
  confidence: number;
  sequenceId: number;
  storedAt: number;
}

function storedTranscript(meetingId: string, transcript: RecoveryTranscript, fallbackSequence: number): StoredTranscript {
  return {
    ...transcript,
    meetingId,
    confidence: transcript.confidence ?? 0,
    sequenceId: transcript.sequence_id ?? transcript.sequenceId ?? fallbackSequence,
    storedAt: Date.now(),
  };
}

/** Schedule cursor work synchronously in its request event, keeping the transaction active. */
function deleteTranscripts(store: IDBObjectStore, meetingId: string, then?: () => void): void {
  const request = store.index('meetingId').openCursor(IDBKeyRange.only(meetingId));
  request.onsuccess = () => {
    const cursor = request.result;
    if (cursor) {
      cursor.delete();
      cursor.continue();
    } else {
      then?.();
    }
  };
}

export class IndexedDBService {
  private db: IDBDatabase | null = null;
  private initPromise: Promise<void> | null = null;

  constructor(private readonly factory?: IDBFactory) {}

  async init(): Promise<void> {
    if (this.db) return;
    if (this.initPromise) return this.initPromise;
    this.initPromise = new Promise<void>((resolve, reject) => {
      const request = (this.factory ?? globalThis.indexedDB).open('SonefoldRecoveryDB', 1);
      request.onerror = () => reject(request.error);
      request.onsuccess = () => {
        const db = request.result;
        this.db = db;
        const release = () => {
          db.close();
          if (this.db === db) this.db = null;
        };
        db.onversionchange = release;
        db.onclose = release;
        resolve();
      };
      request.onupgradeneeded = () => {
        const db = request.result;
        if (!db.objectStoreNames.contains('meetings')) {
          const meetings = db.createObjectStore('meetings', { keyPath: 'meetingId' });
          meetings.createIndex('lastUpdated', 'lastUpdated', { unique: false });
          meetings.createIndex('savedToSQLite', 'savedToSQLite', { unique: false });
        }
        if (!db.objectStoreNames.contains('transcripts')) {
          const transcripts = db.createObjectStore('transcripts', { keyPath: 'id', autoIncrement: true });
          transcripts.createIndex('meetingId', 'meetingId', { unique: false });
          transcripts.createIndex('storedAt', 'storedAt', { unique: false });
        }
      };
    }).finally(() => { this.initPromise = null; });
    return this.initPromise;
  }

  /** Work queues requests synchronously; only the transaction decides success or failure. */
  private async transact<T>(
    stores: string[], mode: IDBTransactionMode, initial: T,
    work: (transaction: IDBTransaction, result: (value: T) => void) => void,
  ): Promise<T> {
    await this.init();
    const transaction = this.db!.transaction(stores, mode);
    return new Promise<T>((resolve, reject) => {
      let result = initial;
      transaction.oncomplete = () => resolve(result);
      transaction.onabort = () => reject(transaction.error ?? new DOMException('Recovery transaction aborted', 'AbortError'));
      try {
        work(transaction, value => { result = value; });
      } catch (error) {
        transaction.abort();
        reject(error);
      }
    });
  }

  async saveMeetingMetadata(metadata: MeetingMetadata): Promise<void> {
    try {
      await this.transact(['meetings'], 'readwrite', undefined, tx => { tx.objectStore('meetings').put(metadata); });
    } catch (error) {
      // A recovery checkpoint must not interrupt recording. Destructive operations propagate errors below.
      console.warn('Failed to save meeting metadata to IndexedDB:', error);
    }
  }

  async getMeetingMetadata(meetingId: string): Promise<MeetingMetadata | null> {
    try {
      return await this.transact<MeetingMetadata | null>(['meetings'], 'readonly', null, (tx, result) => {
        const request = tx.objectStore('meetings').get(meetingId);
        request.onsuccess = () => result(request.result ?? null);
      });
    } catch (error) {
      console.error('Failed to get meeting metadata from IndexedDB:', error);
      return null;
    }
  }

  async getAllMeetings(): Promise<MeetingMetadata[]> {
    try {
      return await this.transact<MeetingMetadata[]>(['meetings'], 'readonly', [], (tx, result) => {
        const request = tx.objectStore('meetings').getAll();
        request.onsuccess = () => result((request.result as MeetingMetadata[])
          .filter(meeting => meeting.savedToSQLite === false).sort((a, b) => b.lastUpdated - a.lastUpdated));
      });
    } catch (error) {
      console.error('Failed to get meetings from IndexedDB:', error);
      return [];
    }
  }

  async markMeetingSaved(meetingId: string): Promise<void> {
    try {
      await this.transact(['meetings'], 'readwrite', undefined, tx => {
        const store = tx.objectStore('meetings');
        const request = store.get(meetingId);
        request.onsuccess = () => {
          if (request.result) store.put({ ...request.result, savedToSQLite: true, lastUpdated: Date.now() });
        };
      });
    } catch (error) {
      console.warn('Failed to mark meeting as saved:', error);
    }
  }

  async deleteMeeting(meetingId: string): Promise<void> {
    await this.transact(['meetings', 'transcripts'], 'readwrite', undefined, tx => {
      deleteTranscripts(tx.objectStore('transcripts'), meetingId);
      tx.objectStore('meetings').delete(meetingId);
    });
  }

  /** Atomically replace a cumulative revision, retaining its segment IDs and speaker/timing fields. */
  async replaceTranscriptSnapshot(meetingId: string, transcripts: RecoveryTranscript[]): Promise<void> {
    await this.transact(['transcripts', 'meetings'], 'readwrite', undefined, tx => {
      const store = tx.objectStore('transcripts');
      deleteTranscripts(store, meetingId, () => {
        transcripts.forEach((transcript, index) => store.put(storedTranscript(meetingId, transcript, index)));
        const meetings = tx.objectStore('meetings');
        const request = meetings.get(meetingId);
        request.onsuccess = () => {
          if (request.result) meetings.put({ ...request.result, lastUpdated: Date.now(), transcriptCount: transcripts.length });
        };
      });
    });
  }

  async saveTranscript(meetingId: string, transcript: RecoveryTranscript): Promise<void> {
    try {
      await this.transact(['transcripts', 'meetings'], 'readwrite', undefined, tx => {
        const meetings = tx.objectStore('meetings');
        const request = meetings.get(meetingId);
        request.onsuccess = () => {
          const meeting: MeetingMetadata | undefined = request.result;
          tx.objectStore('transcripts').add(storedTranscript(meetingId, transcript, meeting?.transcriptCount ?? 0));
          if (meeting) meetings.put({ ...meeting, lastUpdated: Date.now(), transcriptCount: meeting.transcriptCount + 1 });
        };
      });
    } catch (error) {
      console.warn('Failed to save transcript to IndexedDB:', error);
    }
  }

  async getTranscripts(meetingId: string): Promise<StoredTranscript[]> {
    try {
      return await this.transact<StoredTranscript[]>(['transcripts'], 'readonly', [], (tx, result) => {
        const request = tx.objectStore('transcripts').index('meetingId').getAll(meetingId);
        request.onsuccess = () => result((request.result as StoredTranscript[])
          .sort((a, b) => (a.sequenceId ?? a.sequence_id ?? 0) - (b.sequenceId ?? b.sequence_id ?? 0)));
      });
    } catch (error) {
      console.error('Failed to get transcripts from IndexedDB:', error);
      return [];
    }
  }

  async deleteOldMeetings(daysOld: number): Promise<number> {
    return this.pruneMeetings(Date.now() - daysOld * 24 * 60 * 60 * 1000, () => true);
  }

  async deleteSavedMeetings(hoursOld: number): Promise<number> {
    return this.pruneMeetings(Date.now() - hoursOld * 60 * 60 * 1000, meeting => meeting.savedToSQLite === true);
  }

  private async pruneMeetings(cutoff: number, eligible: (meeting: MeetingMetadata) => boolean): Promise<number> {
    try {
      return await this.transact(['meetings', 'transcripts'], 'readwrite', 0, (tx, result) => {
        let deleted = 0;
        const request = tx.objectStore('meetings').index('lastUpdated').openCursor(IDBKeyRange.upperBound(cutoff, true));
        request.onsuccess = () => {
          const cursor = request.result;
          if (!cursor) return;
          const meeting: MeetingMetadata = cursor.value;
          if (eligible(meeting)) {
            deleteTranscripts(tx.objectStore('transcripts'), meeting.meetingId);
            cursor.delete();
            result(++deleted);
          }
          cursor.continue();
        };
      });
    } catch (error) {
      console.error('Failed to prune recovery meetings:', error);
      return 0;
    }
  }
}

export const indexedDBService = new IndexedDBService();
