import { notImplemented, srgbToLinear, linearToSrgb } from '@duckfoot/core';
import type { SceneBuffer } from './scene';

export interface RawImage {
  /** Colour-filter-array samples, one per photosite. */
  cfa: Uint16Array;
  pattern: 'RGGB' | 'BGGR' | 'GRBG' | 'GBRG';
  blackLevel: number;
  whiteLevel: number;
  /** Camera RGB to XYZ, row-major 3x3. */
  camMatrix: number[];
  asShotNeutral: [number, number, number];
  width: number;
  height: number;
}

export interface CameraRawMetadata {
  make: string;
  model: string;
  width: number;
  height: number;
  defaultBlack: number;
  defaultWhite: number;
  asShotKelvin?: number;
}

export interface CameraRawDecodeResult {
  scene: SceneBuffer;
  metadata: CameraRawMetadata;
}

/**
 * Camera RAW decoding (CR3, CR2, NEF, ARW, DNG, etc.) using Duckfoot desktop's
 * native Rust pipeline with Rayon and rawloader.
 */
export async function decodeCameraRawNative(
  filePath: string,
  maxEdge = 1600
): Promise<CameraRawDecodeResult> {
  const internals = typeof window !== 'undefined' ? (window as unknown as { __TAURI_INTERNALS__?: { invoke: (cmd: string, args: unknown) => Promise<unknown> } }).__TAURI_INTERNALS__ : undefined;
  if (internals?.invoke) {
    const res = (await internals.invoke('decode_camera_raw', {
      filePath,
      maxEdge,
    })) as {
      metadata: CameraRawMetadata;
      width: number;
      height: number;
      sceneData: number[];
    };

    return {
      scene: {
        data: new Float32Array(res.sceneData),
        width: res.width,
        height: res.height,
        colorspace: 'camera-rgb',
        scale: 1,
      },
      metadata: res.metadata,
    };
  }
  return notImplemented('decodeCameraRawNative', 'native camera RAW decoding requires the Duckfoot desktop application');
}

export function decodeRaw(file: Blob): Promise<RawImage> {
  void file;
  return notImplemented('decodeRaw', 'no RAW container parser in browser runtime; use desktop app for native RAW');
}

/**
 * Lifts an ordinary raster image into the scene-referred pipeline.
 *
 * This is what makes board 2a usable today: undo, scopes, the tone curve, exposure
 * and export all run against real pixel data rather than a placeholder. It is not
 * pretending to be RAW — there is no highlight headroom to recover and demosaic is
 * skipped — but everything it does report is true.
 */
export function decodeRasterAsScene(
  source: ImageBitmap | HTMLImageElement | HTMLCanvasElement,
  maxDimension = 1600
): SceneBuffer {
  const srcW = 'naturalWidth' in source ? source.naturalWidth : source.width;
  const srcH = 'naturalHeight' in source ? source.naturalHeight : source.height;

  // Invariant 4: preview works on downscaled data and must stay interactive.
  const scale = Math.min(1, maxDimension / Math.max(srcW, srcH));
  const width = Math.max(1, Math.round(srcW * scale));
  const height = Math.max(1, Math.round(srcH * scale));

  const canvas = document.createElement('canvas');
  canvas.width = width;
  canvas.height = height;
  const ctx = canvas.getContext('2d', { willReadFrequently: true });
  if (!ctx) throw new Error('Canvas 2D context not available');
  ctx.drawImage(source, 0, 0, width, height);

  const rgba = ctx.getImageData(0, 0, width, height).data;
  const data = new Float32Array(width * height * 3);
  for (let i = 0, j = 0; i < rgba.length; i += 4, j += 3) {
    data[j] = srgbToLinear(rgba[i] / 255);
    data[j + 1] = srgbToLinear(rgba[i + 1] / 255);
    data[j + 2] = srgbToLinear(rgba[i + 2] / 255);
  }

  return { data, width, height, colorspace: 'linear-rec2020', scale };
}

/** Back to gamma-encoded pixels for display. */
export function sceneToImageData(scene: SceneBuffer): ImageData {
  const out = new ImageData(scene.width, scene.height);
  const dst = out.data;
  const src = scene.data;
  const encode = scene.colorspace === 'display' ? (v: number) => v : linearToSrgb;
  for (let i = 0, j = 0; j < src.length; i += 4, j += 3) {
    dst[i] = Math.max(0, Math.min(255, encode(src[j]) * 255));
    dst[i + 1] = Math.max(0, Math.min(255, encode(src[j + 1]) * 255));
    dst[i + 2] = Math.max(0, Math.min(255, encode(src[j + 2]) * 255));
    dst[i + 3] = 255;
  }
  return out;
}
