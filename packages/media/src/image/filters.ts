import type { ImageAdjustments } from '@duckfoot/core';

/**
 * The display-referred raster path.
 *
 * These operate on 8-bit gamma-encoded sRGB and are what the Photo barrel uses for
 * ordinary JPEG/PNG/WebP sources. The scene-referred RAW pipeline lives beside this
 * in `./raw/` and is deliberately NOT connected to it: two documents, two render
 * functions, no adapter. A bridge between them would be a second path to the same
 * pixels, which is exactly the drift DESIGN.md invariant 2 exists to prevent.
 *
 * `ImageAdjustments` and `DEFAULT_ADJUSTMENTS` now live in @duckfoot/core, so a
 * document can be described without importing anything that touches a canvas.
 * They are re-exported from the package root, so existing imports still resolve.
 */

function isIdentity(adjustments: ImageAdjustments): boolean {
  return (
    adjustments.brightness === 0 &&
    adjustments.contrast === 0 &&
    adjustments.saturation === 0 &&
    adjustments.exposure === 0 &&
    adjustments.temperature === 0 &&
    adjustments.vignette === 0
  );
}

/**
 * Applies adjustments, returning new pixel data.
 *
 * Previously this mutated the caller's canvas context in place, which DESIGN.md
 * invariant 5 forbids: "Never mutate a source buffer or bitmap in place. Operations
 * return new data. Because in-place mutation is how 'revert to original' quietly
 * stops working."
 */
export function applyImageAdjustments(
  source: ImageData,
  adjustments: ImageAdjustments
): ImageData {
  const { width, height } = source;
  const output = new ImageData(new Uint8ClampedArray(source.data), width, height);
  if (isIdentity(adjustments)) return output;

  const data = output.data;
  const { brightness, contrast, saturation, exposure, temperature, vignette } = adjustments;

  const b = brightness * 1.5;
  const cFactor = (contrast + 100) / 100;
  const c = cFactor * cFactor;
  const s = (saturation + 100) / 100;
  const e = Math.pow(2, exposure / 50);
  const tempR = temperature > 0 ? 1 + temperature / 150 : 1;
  const tempB = temperature < 0 ? 1 + -temperature / 150 : 1;

  const centerX = width / 2;
  const centerY = height / 2;
  const maxDist = Math.sqrt(centerX * centerX + centerY * centerY);
  const vignetteStrength = vignette / 100;

  for (let i = 0; i < data.length; i += 4) {
    let r = data[i];
    let g = data[i + 1];
    let bSample = data[i + 2];

    r *= e;
    g *= e;
    bSample *= e;

    r += b;
    g += b;
    bSample += b;

    r = (r - 128) * c + 128;
    g = (g - 128) * c + 128;
    bSample = (bSample - 128) * c + 128;

    r *= tempR;
    bSample *= tempB;

    const gray = 0.2989 * r + 0.587 * g + 0.114 * bSample;
    r = gray + (r - gray) * s;
    g = gray + (g - gray) * s;
    bSample = gray + (bSample - gray) * s;

    if (vignetteStrength > 0) {
      const pixelIndex = i / 4;
      const x = (pixelIndex % width) - centerX;
      const y = Math.floor(pixelIndex / width) - centerY;
      const dist = Math.sqrt(x * x + y * y) / maxDist;
      const vig = Math.max(0, 1 - dist * vignetteStrength);
      r *= vig;
      g *= vig;
      bSample *= vig;
    }

    data[i] = Math.min(255, Math.max(0, r));
    data[i + 1] = Math.min(255, Math.max(0, g));
    data[i + 2] = Math.min(255, Math.max(0, bSample));
  }

  return output;
}

/**
 * Convenience for the common case: read from a context, adjust, write back.
 *
 * The context is still written to — it is the render target, not the source — but
 * the pixel data the caller holds is never touched.
 */
export function drawAdjusted(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  adjustments: ImageAdjustments
): void {
  if (isIdentity(adjustments)) return;
  const source = ctx.getImageData(0, 0, width, height);
  ctx.putImageData(applyImageAdjustments(source, adjustments), 0, 0);
}

export function cropCanvas(
  source: HTMLCanvasElement | ImageBitmap | HTMLImageElement,
  x: number,
  y: number,
  width: number,
  height: number
): HTMLCanvasElement {
  const canvas = document.createElement('canvas');
  canvas.width = Math.max(1, Math.round(width));
  canvas.height = Math.max(1, Math.round(height));
  const ctx = canvas.getContext('2d');
  if (!ctx) throw new Error('Canvas 2D context not available');

  ctx.drawImage(source, x, y, width, height, 0, 0, canvas.width, canvas.height);
  return canvas;
}

export function resizeCanvas(
  source: HTMLCanvasElement | ImageBitmap | HTMLImageElement,
  targetWidth: number,
  targetHeight: number
): HTMLCanvasElement {
  const canvas = document.createElement('canvas');
  canvas.width = Math.max(1, Math.round(targetWidth));
  canvas.height = Math.max(1, Math.round(targetHeight));
  const ctx = canvas.getContext('2d');
  if (!ctx) throw new Error('Canvas 2D context not available');

  ctx.imageSmoothingEnabled = true;
  ctx.imageSmoothingQuality = 'high';
  ctx.drawImage(source, 0, 0, canvas.width, canvas.height);
  return canvas;
}
