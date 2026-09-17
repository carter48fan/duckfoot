export type MediaType = 'photo' | 'audio' | 'video';

/**
 * Which barrel is in front. DESIGN.md's noun is "barrel", not "tool" — the word
 * carries the product thesis (three purpose-built barrels, one stock), so the code
 * uses it too.
 */
export type BarrelId = 'photo' | 'audio' | 'video';

export interface AssetItem {
  id: string;
  name: string;
  type: MediaType;
  url: string;
  file?: File;
  size: number;
  duration?: number;
  width?: number;
  height?: number;
  thumbnail?: string;
  createdAt: number;
}

export interface StorageUsage {
  usedBytes: number;
  quotaBytes?: number;
}

export interface StorageDriver {
  /** Identifies the backing store in the status bar. */
  readonly name: 'memory' | 'opfs' | 'tauri-fs';
  saveFile(key: string, data: Blob | ArrayBuffer): Promise<string>;
  readFile(key: string): Promise<Blob>;
  deleteFile(key: string): Promise<void>;
  listFiles(): Promise<string[]>;
  /**
   * Real consumption, for the header's storage readout.
   *
   * Returns null when the platform will not say. The status bar then shows an
   * em dash rather than a number — a storage figure the app cannot actually
   * measure is the kind of decorative fiction invariant 7 rules out.
   */
  usage(): Promise<StorageUsage | null>;
}

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}
