export type MediaType = 'photo' | 'audio' | 'video';

export type ActiveTool = 'photo' | 'audio' | 'video';

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

export interface StorageDriver {
  saveFile(key: string, data: Blob | ArrayBuffer): Promise<string>;
  readFile(key: string): Promise<Blob>;
  deleteFile(key: string): Promise<void>;
  listFiles(): Promise<string[]>;
}

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}
