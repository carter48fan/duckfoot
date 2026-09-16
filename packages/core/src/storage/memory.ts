import type { StorageDriver } from '../types';

export class MemoryStorageDriver implements StorageDriver {
  private storage = new Map<string, Blob>();

  async saveFile(key: string, data: Blob | ArrayBuffer): Promise<string> {
    const blob = data instanceof Blob ? data : new Blob([data]);
    this.storage.set(key, blob);
    return key;
  }

  async readFile(key: string): Promise<Blob> {
    const blob = this.storage.get(key);
    if (!blob) throw new Error(`File not found: ${key}`);
    return blob;
  }

  async deleteFile(key: string): Promise<void> {
    this.storage.delete(key);
  }

  async listFiles(): Promise<string[]> {
    return Array.from(this.storage.keys());
  }
}
