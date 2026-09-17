export interface RGB {
  r: number;
  g: number;
  b: number;
}

export function hexToRgb(hex: string): RGB | null {
  const cleanHex = hex.replace('#', '');
  if (cleanHex.length === 3) {
    return {
      r: parseInt(cleanHex[0] + cleanHex[0], 16),
      g: parseInt(cleanHex[1] + cleanHex[1], 16),
      b: parseInt(cleanHex[2] + cleanHex[2], 16),
    };
  }
  if (cleanHex.length === 6) {
    return {
      r: parseInt(cleanHex.substring(0, 2), 16),
      g: parseInt(cleanHex.substring(2, 4), 16),
      b: parseInt(cleanHex.substring(4, 6), 16),
    };
  }
  return null;
}

export function rgbToHex(r: number, g: number, b: number): string {
  const clampR = Math.min(255, Math.max(0, Math.round(r)));
  const clampG = Math.min(255, Math.max(0, Math.round(g)));
  const clampB = Math.min(255, Math.max(0, Math.round(b)));
  return `#${clampR.toString(16).padStart(2, '0')}${clampG.toString(16).padStart(2, '0')}${clampB.toString(16).padStart(2, '0')}`;
}

// ---------------------------------------------------------------------------
// Scene-referred colour. Needed by the Photo barrel's cursor readout, which shows
// L/a/b and a clipping percentage under the pointer.
// ---------------------------------------------------------------------------

export interface Lab {
  l: number;
  a: number;
  b: number;
}

/** D65 white point, matching sRGB and Display P3. */
const WHITE_X = 0.95047;
const WHITE_Y = 1.0;
const WHITE_Z = 1.08883;

/** sRGB primaries, linear light in, CIE XYZ out. */
export function linearRgbToXyz(r: number, g: number, b: number): [number, number, number] {
  return [
    r * 0.4124564 + g * 0.3575761 + b * 0.1804375,
    r * 0.2126729 + g * 0.7151522 + b * 0.072175,
    r * 0.0193339 + g * 0.119192 + b * 0.9503041,
  ];
}

function labFinv(t: number): number {
  // CIE piecewise transfer, avoiding the infinite slope of a plain cube root at 0.
  return t > 0.008856 ? Math.cbrt(t) : 7.787 * t + 16 / 116;
}

export function xyzToLab(x: number, y: number, z: number): Lab {
  const fx = labFinv(x / WHITE_X);
  const fy = labFinv(y / WHITE_Y);
  const fz = labFinv(z / WHITE_Z);
  return { l: 116 * fy - 16, a: 500 * (fx - fy), b: 200 * (fy - fz) };
}

/** Convenience for the pixel-probe readout. Expects linear-light channels in 0..1. */
export function linearRgbToLab(r: number, g: number, b: number): Lab {
  const [x, y, z] = linearRgbToXyz(r, g, b);
  return xyzToLab(x, y, z);
}

/** sRGB electro-optical transfer function: gamma-encoded 0..1 to linear light. */
export function srgbToLinear(channel: number): number {
  return channel <= 0.04045 ? channel / 12.92 : Math.pow((channel + 0.055) / 1.055, 2.4);
}

/** Inverse of `srgbToLinear`. */
export function linearToSrgb(channel: number): number {
  return channel <= 0.0031308 ? channel * 12.92 : 1.055 * Math.pow(channel, 1 / 2.4) - 0.055;
}
