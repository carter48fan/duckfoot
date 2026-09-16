export interface ImageAdjustments {
  brightness: number;  // -100 to 100 (0 default)
  contrast: number;    // -100 to 100 (0 default)
  saturation: number;  // -100 to 100 (0 default)
  exposure: number;    // -100 to 100 (0 default)
  temperature: number; // -100 to 100 (0 default, warm < 0, cool > 0)
  vignette: number;    // 0 to 100 (0 default)
}

export const DEFAULT_ADJUSTMENTS: ImageAdjustments = {
  brightness: 0,
  contrast: 0,
  saturation: 0,
  exposure: 0,
  temperature: 0,
  vignette: 0,
};

export function applyImageAdjustments(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  adjustments: ImageAdjustments
): void {
  const { brightness, contrast, saturation, exposure, temperature, vignette } = adjustments;
  const isIdentity =
    brightness === 0 &&
    contrast === 0 &&
    saturation === 0 &&
    exposure === 0 &&
    temperature === 0 &&
    vignette === 0;

  if (isIdentity) return;

  const imageData = ctx.getImageData(0, 0, width, height);
  const data = imageData.data;

  // Pre-calculate adjustment multipliers
  const b = brightness * 1.5;
  const cFactor = (contrast + 100) / 100;
  const c = cFactor * cFactor;
  const s = (saturation + 100) / 100;
  const e = Math.pow(2, exposure / 50);
  const tempR = temperature > 0 ? 1 + (temperature / 150) : 1;
  const tempB = temperature < 0 ? 1 + (-temperature / 150) : 1;

  const centerX = width / 2;
  const centerY = height / 2;
  const maxDist = Math.sqrt(centerX * centerX + centerY * centerY);
  const vignetteStrength = vignette / 100;

  for (let i = 0; i < data.length; i += 4) {
    let r = data[i];
    let g = data[i + 1];
    let bSample = data[i + 2];

    // Exposure
    r *= e;
    g *= e;
    bSample *= e;

    // Brightness
    r += b;
    g += b;
    bSample += b;

    // Contrast
    r = (r - 128) * c + 128;
    g = (g - 128) * c + 128;
    bSample = (bSample - 128) * c + 128;

    // Temperature
    r *= tempR;
    bSample *= tempB;

    // Saturation
    const gray = 0.2989 * r + 0.587 * g + 0.114 * bSample;
    r = gray + (r - gray) * s;
    g = gray + (g - gray) * s;
    bSample = gray + (bSample - gray) * s;

    // Vignette
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

  ctx.putImageData(imageData, 0, 0);
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
