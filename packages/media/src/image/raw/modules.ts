import { notImplemented } from '@duckfoot/core';
import type {
  CurveNode,
  RawModuleId,
  RawModuleParamMap,
  Rgb4Way,
  ToneCurveChannel,
} from '@duckfoot/core';
import type { SceneBuffer } from './scene';
import { likeScene, mapPixels } from './scene';

/**
 * Module implementations, keyed by id.
 *
 * Presentation metadata (label, defaults, the `summary()` readout) lives in
 * @duckfoot/core alongside the document types — a module's name and its formatted
 * value are needed to describe an edit, and describing an edit must not require
 * importing a pixel pipeline.
 *
 * A module marked `status: 'stub'` in core is never called by `renderRawStack`; it
 * is skipped and reported. Calling one directly throws, which is the point — see
 * `@duckfoot/core`'s stub.ts for why "returns its input unchanged" is banned.
 */
export type RawModuleApply<K extends RawModuleId = RawModuleId> = (
  scene: SceneBuffer,
  params: RawModuleParamMap[K]
) => SceneBuffer;

// --- tone curve -------------------------------------------------------------

/**
 * Monotone cubic (Fritsch–Carlson) through the curve nodes.
 *
 * A plain Catmull–Rom spline overshoots between close control points, which on a
 * tone curve shows up as a reversal — the image gets darker as you drag a node up.
 * Monotone interpolation cannot do that.
 */
function buildCurveLut(nodes: CurveNode[], size = 1024): Float32Array {
  const pts = [...nodes].sort((a, b) => a.x - b.x);
  const lut = new Float32Array(size);
  if (pts.length < 2) {
    for (let i = 0; i < size; i++) lut[i] = i / (size - 1);
    return lut;
  }

  const n = pts.length;
  const dx: number[] = [];
  const slope: number[] = [];
  for (let i = 0; i < n - 1; i++) {
    const h = pts[i + 1].x - pts[i].x;
    dx.push(h);
    slope.push(h === 0 ? 0 : (pts[i + 1].y - pts[i].y) / h);
  }

  const tangent: number[] = new Array(n).fill(0);
  tangent[0] = slope[0];
  tangent[n - 1] = slope[n - 2];
  for (let i = 1; i < n - 1; i++) {
    if (slope[i - 1] * slope[i] <= 0) {
      tangent[i] = 0;
    } else {
      tangent[i] = (slope[i - 1] + slope[i]) / 2;
      const limit = 3 * Math.min(Math.abs(slope[i - 1]), Math.abs(slope[i]));
      if (Math.abs(tangent[i]) > limit) tangent[i] = Math.sign(tangent[i]) * limit;
    }
  }

  let seg = 0;
  for (let i = 0; i < size; i++) {
    const x = i / (size - 1);
    while (seg < n - 2 && x > pts[seg + 1].x) seg++;
    const h = dx[seg];
    if (h === 0) {
      lut[i] = pts[seg].y;
      continue;
    }
    const t = (x - pts[seg].x) / h;
    const t2 = t * t;
    const t3 = t2 * t;
    lut[i] =
      (2 * t3 - 3 * t2 + 1) * pts[seg].y +
      (t3 - 2 * t2 + t) * h * tangent[seg] +
      (-2 * t3 + 3 * t2) * pts[seg + 1].y +
      (t3 - t2) * h * tangent[seg + 1];
  }
  return lut;
}

function sampleLut(lut: Float32Array, value: number): number {
  if (value <= 0) return lut[0];
  if (value >= 1) return lut[lut.length - 1] + (value - 1);
  const pos = value * (lut.length - 1);
  const i = Math.floor(pos);
  const frac = pos - i;
  return lut[i] + (lut[Math.min(i + 1, lut.length - 1)] - lut[i]) * frac;
}

function applyCurveChannel(
  channel: ToneCurveChannel,
  lut: Float32Array,
  r: number,
  g: number,
  b: number
): [number, number, number] {
  switch (channel) {
    case 'r':
      return [sampleLut(lut, r), g, b];
    case 'g':
      return [r, sampleLut(lut, g), b];
    case 'b':
      return [r, g, sampleLut(lut, b)];
    case 'l': {
      // Luminance-only: preserve the ratio between channels so hue does not drift.
      const luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
      if (luma <= 0) return [r, g, b];
      const scale = sampleLut(lut, luma) / luma;
      return [r * scale, g * scale, b * scale];
    }
    case 'rgb':
    default:
      return [sampleLut(lut, r), sampleLut(lut, g), sampleLut(lut, b)];
  }
}

// --- colour balance ---------------------------------------------------------

function balanceChannel(value: number, lift: number, gamma: number, gain: number, offset: number): number {
  // Lift-gamma-gain, the standard four-way grade. Offset is additive in linear light.
  let v = value + offset;
  v = v * (1 + gain) + lift * (1 - v);
  if (v > 0 && gamma !== 0) v = Math.pow(v, 1 / (1 + gamma));
  return v;
}

function four(way: Rgb4Way): [number, number, number] {
  return [way.r, way.g, way.b];
}

// --- crop & rotate ----------------------------------------------------------

function cropRotate(scene: SceneBuffer, params: RawModuleParamMap['cropRotate']): SceneBuffer {
  const { rect, angleDeg } = params;
  const srcW = scene.width;
  const srcH = scene.height;

  // Rect is stored in normalised coordinates so it survives a scale change between
  // preview and export — the same rect must mean the same crop at both resolutions.
  const x0 = rect ? Math.max(0, Math.round(rect.x * srcW)) : 0;
  const y0 = rect ? Math.max(0, Math.round(rect.y * srcH)) : 0;
  const outW = rect ? Math.max(1, Math.round(rect.width * srcW)) : srcW;
  const outH = rect ? Math.max(1, Math.round(rect.height * srcH)) : srcH;

  if (angleDeg === 0 && !rect) return scene;

  const out: SceneBuffer = {
    data: new Float32Array(outW * outH * 3),
    width: outW,
    height: outH,
    colorspace: scene.colorspace,
    scale: scene.scale,
  };

  const theta = (angleDeg * Math.PI) / 180;
  const cos = Math.cos(theta);
  const sin = Math.sin(theta);
  const cx = x0 + outW / 2;
  const cy = y0 + outH / 2;

  for (let y = 0; y < outH; y++) {
    for (let x = 0; x < outW; x++) {
      // Rotate about the crop centre, then sample the source bilinearly.
      const dx = x - outW / 2;
      const dy = y - outH / 2;
      const sx = cx + dx * cos - dy * sin;
      const sy = cy + dx * sin + dy * cos;

      const di = (y * outW + x) * 3;
      if (sx < 0 || sy < 0 || sx >= srcW - 1 || sy >= srcH - 1) {
        out.data[di] = 0;
        out.data[di + 1] = 0;
        out.data[di + 2] = 0;
        continue;
      }

      const ix = Math.floor(sx);
      const iy = Math.floor(sy);
      const fx = sx - ix;
      const fy = sy - iy;
      for (let c = 0; c < 3; c++) {
        const i00 = (iy * srcW + ix) * 3 + c;
        const i10 = (iy * srcW + ix + 1) * 3 + c;
        const i01 = ((iy + 1) * srcW + ix) * 3 + c;
        const i11 = ((iy + 1) * srcW + ix + 1) * 3 + c;
        const top = scene.data[i00] * (1 - fx) + scene.data[i10] * fx;
        const bottom = scene.data[i01] * (1 - fx) + scene.data[i11] * fx;
        out.data[di + c] = top * (1 - fy) + bottom * fy;
      }
    }
  }
  return out;
}

// --- the registry -----------------------------------------------------------

type ApplyMap = { [K in RawModuleId]: RawModuleApply<K> };

export const RAW_MODULE_APPLY: ApplyMap = {
  rawLevels: (scene, p) => {
    // Normalise sensor counts to 0..1. Below black is real information (noise floor),
    // but it is not light, so it clamps.
    const range = p.white - p.black;
    if (range <= 0) return scene;
    const scale = 1 / range;
    const black = p.black / 65535;
    const norm = 65535 * scale;
    return mapPixels(scene, (r, g, b) => [
      Math.max(0, (r - black) * norm),
      Math.max(0, (g - black) * norm),
      Math.max(0, (b - black) * norm),
    ]);
  },

  whiteBalance: (scene, p) => {
    // Van Kries adaptation approximated from correlated colour temperature, with
    // tint pulling the green/magenta axis. Good enough to be useful; a real camera
    // profile would use the as-shot neutral from the file.
    const t = p.temperatureK / 5500;
    const rGain = Math.pow(t, 0.55);
    const bGain = Math.pow(1 / t, 0.55);
    const gGain = 1 - p.tint / 400;
    return mapPixels(scene, (r, g, b) => [r * rGain, g * gGain, b * bGain]);
  },

  demosaic: () => notImplemented('demosaic', 'needs CFA input, which decodeRaw does not yet produce'),

  exposure: (scene, p) => {
    const gain = Math.pow(2, p.ev);
    const black = p.blackLevel;
    return mapPixels(scene, (r, g, b) => [
      (r - black) * gain,
      (g - black) * gain,
      (b - black) * gain,
    ]);
  },

  toneCurve: (scene, p) => {
    const lut = buildCurveLut(p.nodes);
    return mapPixels(scene, (r, g, b) => applyCurveChannel(p.channel, lut, r, g, b));
  },

  filmicRgb: (scene, p) => {
    // A filmic sigmoid in log space: maps scene-referred values with headroom into a
    // display range without the hard shoulder clip that makes highlights look cut off.
    // This is the shape of filmic, not darktable's full implementation.
    const white = Math.pow(2, p.whiteRelEv);
    const black = Math.pow(2, p.blackRelEv);
    const latitude = Math.max(0.01, p.latitude / 100);
    const contrast = p.contrast;
    const logRange = Math.log2(white / black);
    const curve = (v: number): number => {
      if (v <= 0) return 0;
      const norm = (Math.log2(v / black) / logRange - 0.5) * contrast;
      const s = 1 / (1 + Math.exp(-norm / latitude));
      return Math.max(0, Math.min(1, s));
    };
    return mapPixels(scene, (r, g, b) => [curve(r), curve(g), curve(b)], 'display');
  },

  toneEqualizer: () =>
    notImplemented('toneEqualizer', 'needs a guided-filter luminance mask'),

  colorBalanceRgb: (scene, p) => {
    const [lr, lg, lb] = four(p.lift);
    const [gr, gg, gb] = four(p.gamma);
    const [ar, ag, ab] = four(p.gain);
    const [or_, og, ob] = four(p.offset);
    return mapPixels(scene, (r, g, b) => [
      balanceChannel(r, lr, gr, ar, or_),
      balanceChannel(g, lg, gg, ag, og),
      balanceChannel(b, lb, gb, ab, ob),
    ]);
  },

  localContrast: () => notImplemented('localContrast', 'needs a guided or bilateral filter'),

  captureSharpen: () => notImplemented('captureSharpen', 'needs a deconvolution kernel'),

  denoiseProfiled: () =>
    notImplemented('denoiseProfiled', 'needs per-sensor noise profiles'),

  lensCorrection: () => notImplemented('lensCorrection', 'needs a lens profile database'),

  cropRotate,

  outputProfile: (scene, p) => {
    // Primaries conversion is a stub; what is real here is the clamp to the output
    // range, which is what makes the histogram and the clipping readout meaningful.
    void p;
    return mapPixels(
      scene,
      (r, g, b) => [
        Math.max(0, Math.min(1, r)),
        Math.max(0, Math.min(1, g)),
        Math.max(0, Math.min(1, b)),
      ],
      'display'
    );
  },
};

/** Exposed for the tone-curve editor, which draws the same LUT it renders through. */
export { buildCurveLut, sampleLut };

/** Unused re-export guard: keeps `likeScene` reachable for module authors. */
export { likeScene };
