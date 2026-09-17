import type { StorageDriver, StorageUsage } from '../types';

export class OpfsStorageDriver implements StorageDriver {
  readonly name = 'opfs' as const;

  private rootPromise: Promise<FileSystemDirectoryHandle> | null = null;

  private async getRoot(): Promise<FileSystemDirectoryHandle> {
    if (!this.rootPromise) {
      if (typeof navigator !== 'undefined' && navigator.storage && 'getDirectory' in navigator.storage) {
        this.rootPromise = navigator.storage.getDirectory();
      } else {
        throw new Error('OPFS is not supported in this environment');
      }
    }
    return this.rootPromise;
  }

  async saveFile(key: string, data: Blob | ArrayBuffer): Promise<string> {
    const root = await this.getRoot();
    const fileHandle = await root.getFileHandle(key, { create: true });
    const writable = await fileHandle.createWritable();
    await writable.write(data);
    await writable.close();
    return key;
  }

  async readFile(key: string): Promise<Blob> {
    const root = await this.getRoot();
    const fileHandle = await root.getFileHandle(key);
    const file = await fileHandle.getFile();
    return file;
  }

  async deleteFile(key: string): Promise<void> {
    const root = await this.getRoot();
    await root.removeEntry(key);
  }

  async listFiles(): Promise<string[]> {
    const root = await this.getRoot();
    const files: string[] = [];
    // @ts-ignore - values() iterator in modern browsers
    for await (const entry of root.values()) {
      if (entry.kind === 'file') {
        files.push(entry.name);
      }
    }
    return files;
  }

  async usage(): Promise<StorageUsage | null> {
    if (typeof navigator === 'undefined' || !navigator.storage?.estimate) return null;
    const estimate = await navigator.storage.estimate();
    if (typeof estimate.usage !== 'number') return null;
    return { usedBytes: estimate.usage, quotaBytes: estimate.quota };
  }
}
