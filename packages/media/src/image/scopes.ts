import { notImplemented, linearRgbToLab } from '@duckfoot/core';
import type { Lab } from '@duckfoot/core';
import type { SceneBuffer } from './raw/scene';

/**
 * Scopes read the POST-pipeline preview scene, after the output profile.
 *
 * That matters: the board's readouts (`L 74.2  a +3.1  b +11.6`, `clipped 0.4%`)
 * are display-referred claims. Measuring the scene-referred intermediate would give
 * numbers that are precise and about the wrong thing. Because the preview is
 * approximate (invariant 4), the scopes are approximate too — which is correct, and
 * is why the board says "preview is approximate · export re-runs the pipeline".
 *
 * Everything here returns counts, never pixels. Keeping them DOM-free means they
 * move into the export worker unchanged if exact scopes are ever wanted.
 */

export interface Histogram {
  bins: number;
  r: Uint32Array;
  g: Uint32Array;
  b: Uint32Array;
  luma: Uint32Array;
  /** Fraction of samples at or below 0 — the board's shadow clipping. */
  clippedLowFraction: number;
  /** Fraction at or above 1 — the board's "clipped 0.4%". */
  clippedHighFraction: number;
}

export function computeHistogram(scene: SceneBuffer, bins = 256): Histogram {
  const r = new Uint32Array(bins);
  const g = new Uint32Array(bins);
  const b = new Uint32Array(bins);
  const luma = new Uint32Array(bins);
  const data = scene.data;
  const last = bins - 1;

  let clippedLow = 0;
  let clippedHigh = 0;
  const pixels = data.length / 3;

  for (let i = 0; i < data.length; i += 3) {
    const cr = data[i];
    const cg = data[i + 1];
    const cb = data[i + 2];

    r[Math.max(0, Math.min(last, Math.round(cr * last)))]++;
    g[Math.max(0, Math.min(last, Math.round(cg * last)))]++;
    b[Math.max(0, Math.min(last, Math.round(cb * last)))]++;

    const y = 0.2126 * cr + 0.7152 * cg + 0.0722 * cb;
    luma[Math.max(0, Math.min(last, Math.round(y * last)))]++;

    if (cr >= 1 || cg >= 1 || cb >= 1) clippedHigh++;
    else if (cr <= 0 && cg <= 0 && cb <= 0) clippedLow++;
  }

  return {
    bins,
    r,
    g,
    b,
    luma,
    clippedLowFraction: pixels === 0 ? 0 : clippedLow / pixels,
    clippedHighFraction: pixels === 0 ? 0 : clippedHigh / pixels,
  };
}

export interface PixelSample {
  rgb: [number, number, number];
  lab: Lab;
  clipped: boolean;
}

/** The cursor readout over the image. */
export function pickSample(scene: SceneBuffer, x: number, y: number): PixelSample | null {
  const px = Math.round(x);
  const py = Math.round(y);
  if (px < 0 || py < 0 || px >= scene.width || py >= scene.height) return null;
  const i = (py * scene.width + px) * 3;
  const r = scene.data[i];
  const g = scene.data[i + 1];
  const b = scene.data[i + 2];
  return { rgb: [r, g, b], lab: linearRgbToLab(r, g, b), clipped: r >= 1 || g >= 1 || b >= 1 };
}

export interface WaveformScope {
  columns: number;
  rows: number;
  r: Uint16Array;
  g: Uint16Array;
  b: Uint16Array;
}

/**
 * @stub RGB parade waveform.
 *
 * The histogram covers the question most people ask of a scope; the parade is a
 * second display of data the pipeline already produces, so it is deferred rather
 * than faked. The panel renders an explicit placeholder.
 */
export function computeWaveform(scene: SceneBuffer, columns = 256, rows = 256): WaveformScope {
  void scene;
  void columns;
  void rows;
  return notImplemented('computeWaveform', 'scope panel is deferred to a later turn');
}

export interface VectorscopeData {
  size: number;
  density: Uint16Array;
}

/** @stub CbCr vectorscope. Deferred with the waveform, for the same reason. */
export function computeVectorscope(scene: SceneBuffer, size = 256): VectorscopeData {
  void scene;
  void size;
  return notImplemented('computeVectorscope', 'scope panel is deferred to a later turn');
}
