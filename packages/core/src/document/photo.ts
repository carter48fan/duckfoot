import type { Rect } from '../types';
import type { ImplementationStatus } from '../stub';

/**
 * Display-referred adjustments for raster sources (JPEG, PNG, WebP).
 *
 * Deliberately distinct from the scene-referred RAW stack below. These operate on
 * 8-bit sRGB pixel values; the RAW stack operates on linear float. They are two
 * documents, not two implementations of one thing, and there is intentionally NO
 * adapter between them — a second path to the same pixels is exactly the drift
 * DESIGN.md invariant 2 exists to prevent.
 *
 * Moved here from @duckfoot/media so that a PhotoDocument can be described without
 * importing anything that touches a canvas.
 */
export interface ImageAdjustments {
  brightness: number;
  contrast: number;
  saturation: number;
  exposure: number;
  temperature: number;
  vignette: number;
}

export const DEFAULT_ADJUSTMENTS: ImageAdjustments = {
  brightness: 0,
  contrast: 0,
  saturation: 0,
  exposure: 0,
  temperature: 0,
  vignette: 0,
};

/**
 * The scene-referred RAW pipeline.
 *
 * Order is fixed by the product and lives in RAW_MODULE_ORDER, not in the document.
 * That is the line against becoming a node graph: modules switch on and off, they
 * never reorder. If reorderable stacks are ever wanted, add an explicit `order`
 * field then — do not pre-build it now.
 */
export type RawModuleId =
  | 'rawLevels'
  | 'whiteBalance'
  | 'demosaic'
  | 'exposure'
  | 'toneCurve'
  | 'filmicRgb'
  | 'toneEqualizer'
  | 'colorBalanceRgb'
  | 'localContrast'
  | 'captureSharpen'
  | 'denoiseProfiled'
  | 'lensCorrection'
  | 'cropRotate'
  | 'outputProfile';

/** Processing order, top to bottom as the Inspector renders it. */
export const RAW_MODULE_ORDER: readonly RawModuleId[] = [
  'rawLevels',
  'whiteBalance',
  'demosaic',
  'exposure',
  'toneCurve',
  'filmicRgb',
  'toneEqualizer',
  'colorBalanceRgb',
  'localContrast',
  'captureSharpen',
  'denoiseProfiled',
  'lensCorrection',
  'cropRotate',
  'outputProfile',
];

export interface CurveNode {
  x: number;
  y: number;
}

export type ToneCurveChannel = 'rgb' | 'r' | 'g' | 'b' | 'l';
export type DemosaicAlgorithm = 'rcd' | 'ppg' | 'bilinear';
export type OutputColorSpace = 'srgb' | 'display-p3' | 'prophoto' | 'rec2020';

export interface Rgb4Way {
  r: number;
  g: number;
  b: number;
}

export interface RawModuleParamMap {
  rawLevels: { black: number; white: number };
  whiteBalance: { temperatureK: number; tint: number };
  demosaic: { algorithm: DemosaicAlgorithm };
  exposure: { ev: number; blackLevel: number };
  toneCurve: { channel: ToneCurveChannel; nodes: CurveNode[] };
  filmicRgb: { whiteRelEv: number; blackRelEv: number; latitude: number; contrast: number };
  toneEqualizer: { bandsDb: number[] };
  colorBalanceRgb: { lift: Rgb4Way; gamma: Rgb4Way; gain: Rgb4Way; offset: Rgb4Way };
  localContrast: { detail: number; radiusPx: number };
  captureSharpen: { radiusPx: number; amount: number };
  denoiseProfiled: { luma: number; chroma: number };
  lensCorrection: { profileId: string | null; distortion: number; vignetting: number; ca: number };
  cropRotate: { aspect: string | null; angleDeg: number; rect: Rect | null };
  outputProfile: { profile: OutputColorSpace; bitDepth: 8 | 16 };
}

export interface RawModuleInstance<K extends RawModuleId = RawModuleId> {
  id: K;
  enabled: boolean;
  params: RawModuleParamMap[K];
}

/**
 * Keyed by id rather than an array: order is a constant, so the document only needs
 * O(1) typed access (`stack.exposure.params.ev`), stable serialisation and cheap
 * diffing for undo.
 */
export type RawStack = { [K in RawModuleId]: RawModuleInstance<K> };

/**
 * Presentation metadata for a module — everything the Inspector and the HISTORY
 * panel need, with no dependency on pixels.
 *
 * `summary` produces the readout in the board's right-hand column. Keeping it here
 * rather than in JSX is what lets the Inspector render all fourteen rows from one
 * template while hardcoding no units, and what guarantees the HISTORY label and the
 * module label can never disagree.
 */
export interface RawModuleMeta<K extends RawModuleId = RawModuleId> {
  id: K;
  /** Lowercase, exactly as the board renders it. */
  label: string;
  defaults: RawModuleParamMap[K];
  defaultEnabled: boolean;
  /** Changes image geometry, so scopes and overlays must re-fit after it. */
  geometric?: boolean;
  status: ImplementationStatus;
  summary(params: RawModuleParamMap[K], enabled: boolean): string;
}

/** U+2212 MINUS SIGN, matching the design board's numerals. */
const MINUS = '−';

function signed(value: number, digits = 0): string {
  const fixed = Math.abs(value).toFixed(digits);
  if (value < 0) return `${MINUS}${fixed}`;
  return `+${fixed}`;
}

type MetaMap = { [K in RawModuleId]: RawModuleMeta<K> };

export const RAW_MODULE_META: MetaMap = {
  rawLevels: {
    id: 'rawLevels',
    label: 'raw black / white point',
    defaults: { black: 2048, white: 15360 },
    defaultEnabled: true,
    status: 'implemented',
    summary: (p) => `${p.black} · ${p.white}`,
  },
  whiteBalance: {
    id: 'whiteBalance',
    label: 'white balance',
    defaults: { temperatureK: 5240, tint: 7 },
    defaultEnabled: true,
    status: 'implemented',
    summary: (p) => `${p.temperatureK} K · ${signed(p.tint)} tint`,
  },
  demosaic: {
    id: 'demosaic',
    label: 'demosaic',
    defaults: { algorithm: 'rcd' },
    defaultEnabled: true,
    status: 'stub',
    summary: (p) => p.algorithm.toUpperCase(),
  },
  exposure: {
    id: 'exposure',
    label: 'exposure',
    defaults: { ev: 0.42, blackLevel: 0 },
    defaultEnabled: true,
    status: 'implemented',
    summary: (p) => `${signed(p.ev, 2)} EV`,
  },
  toneCurve: {
    id: 'toneCurve',
    label: 'tone curve',
    defaults: {
      channel: 'rgb',
      nodes: [
        { x: 0, y: 0.02 },
        { x: 0.32, y: 0.36 },
        { x: 0.68, y: 0.76 },
        { x: 1, y: 0.97 },
      ],
    },
    defaultEnabled: true,
    status: 'implemented',
    summary: (p) => (p.channel === 'rgb' ? 'RGB linked' : p.channel.toUpperCase()),
  },
  filmicRgb: {
    id: 'filmicRgb',
    label: 'filmic rgb',
    defaults: { whiteRelEv: 4, blackRelEv: -8, latitude: 25, contrast: 1.35 },
    defaultEnabled: true,
    status: 'partial',
    summary: (p) => `latitude ${p.latitude}%`,
  },
  toneEqualizer: {
    id: 'toneEqualizer',
    label: 'tone equalizer',
    defaults: { bandsDb: [0, 0, 0, 0, 0, 0, 0, 0] },
    defaultEnabled: true,
    status: 'stub',
    summary: (p) => `${p.bandsDb.length} bands`,
  },
  colorBalanceRgb: {
    id: 'colorBalanceRgb',
    label: 'color balance rgb',
    defaults: {
      lift: { r: 0, g: 0, b: 0 },
      gamma: { r: 0, g: 0, b: 0 },
      gain: { r: 0, g: 0, b: 0 },
      offset: { r: 0, g: 0, b: 0 },
    },
    defaultEnabled: true,
    status: 'implemented',
    summary: () => '4-way',
  },
  localContrast: {
    id: 'localContrast',
    label: 'local contrast',
    defaults: { detail: 32, radiusPx: 8 },
    defaultEnabled: true,
    status: 'stub',
    summary: (p) => `detail ${signed(p.detail)}`,
  },
  captureSharpen: {
    id: 'captureSharpen',
    label: 'capture sharpening',
    defaults: { radiusPx: 0.7, amount: 0.5 },
    defaultEnabled: true,
    status: 'stub',
    summary: (p) => `${p.radiusPx.toFixed(1)} px`,
  },
  denoiseProfiled: {
    id: 'denoiseProfiled',
    label: 'denoise (profiled)',
    defaults: { luma: 0, chroma: 0 },
    defaultEnabled: false,
    status: 'stub',
    summary: (_p, enabled) => (enabled ? 'profiled' : 'off'),
  },
  lensCorrection: {
    id: 'lensCorrection',
    label: 'lens correction',
    defaults: { profileId: null, distortion: 0, vignetting: 0, ca: 0 },
    defaultEnabled: false,
    geometric: true,
    status: 'stub',
    summary: (p) => (p.profileId ? p.profileId : 'no profile'),
  },
  cropRotate: {
    id: 'cropRotate',
    label: 'crop & rotate',
    defaults: { aspect: '4:5', angleDeg: -0.6, rect: null },
    defaultEnabled: true,
    geometric: true,
    status: 'implemented',
    summary: (p) => {
      const angle = `${p.angleDeg < 0 ? MINUS : ''}${Math.abs(p.angleDeg).toFixed(1)}°`;
      return p.aspect ? `${p.aspect} · ${angle}` : angle;
    },
  },
  outputProfile: {
    id: 'outputProfile',
    label: 'output profile',
    defaults: { profile: 'prophoto', bitDepth: 16 },
    defaultEnabled: true,
    status: 'implemented',
    summary: (p) => OUTPUT_PROFILE_LABELS[p.profile],
  },
};

const OUTPUT_PROFILE_LABELS: Record<OutputColorSpace, string> = {
  srgb: 'sRGB',
  'display-p3': 'Display P3',
  prophoto: 'ProPhoto',
  rec2020: 'Rec. 2020',
};

export function createRawStack(): RawStack {
  const stack = {} as RawStack;
  for (const id of RAW_MODULE_ORDER) {
    const meta = RAW_MODULE_META[id];
    // Structured-clone the defaults so two documents never share a params object —
    // the same reasoning as invariant 5, one level up.
    (stack as Record<string, RawModuleInstance>)[id] = {
      id,
      enabled: meta.defaultEnabled,
      params: structuredClone(meta.defaults),
    } as RawModuleInstance;
  }
  return stack;
}

/** The board's "9 modules on". */
export function enabledModuleCount(stack: RawStack): number {
  return RAW_MODULE_ORDER.filter((id) => stack[id].enabled).length;
}

export function moduleSummary(stack: RawStack, id: RawModuleId): string {
  const instance = stack[id];
  const meta = RAW_MODULE_META[id] as RawModuleMeta;
  return meta.summary(instance.params, instance.enabled);
}

export type PhotoSourceKind = 'raster' | 'raw';

export interface PhotoDocument {
  kind: PhotoSourceKind;
  assetId: string | null;
  /** Used when kind === 'raw'. */
  stack: RawStack;
  /** Used when kind === 'raster'. */
  adjustments: ImageAdjustments;
  watermark: string;
}

export function createPhotoDocument(): PhotoDocument {
  return {
    kind: 'raw',
    assetId: null,
    stack: createRawStack(),
    adjustments: { ...DEFAULT_ADJUSTMENTS },
    watermark: '',
  };
}
