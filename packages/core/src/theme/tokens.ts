/**
 * Duckfoot design tokens — the single source of truth for palette, metrics and type.
 *
 * Why this is TypeScript and not a stylesheet: canvas and SVG need literal colour
 * values at draw time (waveform fill, the three histogram channels, the playhead
 * stroke, envelope polylines). If the source of truth were CSS, every draw loop
 * would need `getComputedStyle` or a second hardcoded copy that silently drifts.
 * Going the other way is cheap — `tokensToCssText()` emits the custom properties,
 * and every stylesheet references `var(--df-*)` rather than a literal.
 *
 * Values are taken from the turn-2 design boards (`Duckfoot Barrels.dc.html`, 2a/2b).
 * DESIGN.md describes the palette in prose; this file is that prose made exact.
 */

/** Warm brass on ebony. DESIGN.md: one accent, used sparingly — "not indigo". */
export const color = {
  /** Page backdrop behind the app frame. */
  ebony: '#08080a',
  /** The app frame itself. */
  frame: '#0a0a0a',
  /** Shelf and Inspector panels. */
  surface: '#0f0f0f',
  /** Header and transport — warmer, to read as walnut against the panels. */
  surfaceWarm: '#0f0d0a',
  /** The barrel workspace. */
  canvas: '#0c0c0c',
  /** Wells that hold media: lane backgrounds, scope panels. */
  well: '#101010',
  /** Deeper wells: histogram, tone curve, gain envelope. */
  wellDeep: '#0b0b0b',
  /** Inset controls — switcher track, thumbnails. */
  inset: '#151515',
  /** Thumbnail and chip fill. */
  chip: '#1c1c1c',
  /** Expanded-module background. */
  raised: '#141414',

  /** The accent. */
  brass: '#c9992e',
  /** Hover / active accent. */
  brassLit: '#e0b254',
  /** Recessed accent — outer knurl tines, secondary marks. */
  brassDim: '#8a6b23',
  /** Serif panel headings. */
  brassLabel: '#a08a52',
  /** Accent-tinted borders. */
  brassEdge: '#3a2f14',
  /** Knurl mid-tone. */
  brassDeep: '#7a5f22',
  /** Knurl shadow. */
  brassShadow: '#1a150c',
  /** Accent-tinted surface — active tab, board chips. */
  brassSurface: '#241f16',
  /** Deepest accent surface. */
  brassSurfaceDeep: '#1a1509',
  /** Wordmark. */
  wordmark: '#f0e6d2',
  /** Tagline under the wordmark. */
  tagline: '#8a7a5c',

  /** Borders, lightest to faintest. */
  frameBorder: '#3a3226',
  rule: '#262626',
  ruleSoft: '#1f1f1f',
  ruleDim: '#1c1c1c',
  ruleFaint: '#171717',
  control: '#2a2a2a',

  /** Text ramp. */
  text: '#f5f5f5',
  textBright: '#e5e5e5',
  textCalm: '#d4d4d4',
  textMuted: '#bdbdbd',
  text2: '#a3a3a3',
  text3: '#8a8a8a',
  text4: '#6e6e6e',
  text5: '#575757',
  textFaint: '#4a4a4a',
  textGhost: '#3f3f3f',
  disabled: '#3a3a3a',

  /**
   * Semantic colour. DESIGN.md: reserved, never decorative.
   * green = in-point, red = out-point and destructive, white = playhead,
   * accent wash = selection.
   */
  inPoint: '#4f9d5d',
  outPoint: '#c0473a',
  playhead: '#ffffff',
  destructiveEdge: '#3a2523',
  destructiveText: '#e2a29b',
  destructiveDim: '#7a4a44',

  /** Scope channels — screen-blended in the histogram. */
  scopeR: '#c0473a',
  scopeG: '#4f9d5d',
  scopeB: '#3f6fa8',

  /** Waveform ink: active lane, inactive lane, video clip. */
  wave: '#cfcfcf',
  waveDim: '#5c5c5c',
  waveClip: '#7d7d7d',
} as const;

/** Translucent fills. Kept separate because they are `rgba`, not hex, and canvas needs both. */
export const wash = {
  /** Selection wash across lanes and shelf rows. */
  selection: 'rgba(201,153,46,0.07)',
  /** Selection wash over a lane waveform. */
  selectionLane: 'rgba(201,153,46,0.08)',
  /** Selection wash under the gain envelope. */
  selectionEnvelope: 'rgba(201,153,46,0.06)',
  /** Active toggle fill. */
  accentFill: 'rgba(201,153,46,0.14)',
  /** Accent chip fill. */
  accentChip: 'rgba(201,153,46,0.1)',
  /** Destructive row fill. */
  destructive: 'rgba(192,71,58,0.08)',
  /** Overlay badges sitting on media. */
  badge: 'rgba(10,10,10,0.85)',
  /** Readout strip over the image. */
  readout: 'rgba(10,10,10,0.82)',
  /** Crop mask. */
  cropMask: 'rgba(10,10,10,0.62)',
} as const;

/**
 * Structural metrics, in px. These are the numbers a reviewer checks against the
 * board in devtools, which is why they are tokens and not magic numbers in CSS.
 */
export const metric = {
  /** The knurled brass rule across the top of the frame. */
  rule: 6,
  /** Top chrome: brand, barrel switcher, status. */
  header: 46,
  /** Bottom chrome: barrel actions. */
  transport: 52,
  /** Shelf, expanded. */
  shelf: 216,
  /** Shelf, collapsed to a labelled rail (board 1a). */
  shelfRail: 30,
  /** Inspector. */
  inspector: 318,
  /** Workspace toolbar under the header. */
  toolbar: 36,
  /** Shelf / Inspector section header. */
  sectionHeader: 30,
  /** Sub-panel header inside the Shelf. */
  panelHeader: 28,
  /** Scope panel (histogram / waveform / vectorscope). */
  scope: 74,
  /** Tone curve editor. */
  toneCurve: 104,
  /** Photo filmstrip strip. */
  filmstrip: 92,
  /** Filmstrip frame. */
  filmstripFrame: 88,
  /** Audio lane label column. */
  laneLabel: 110,
  /** Audio lane control column (gain, polarity, mute, solo). */
  laneCtl: 196,
  /** Gain envelope lane. */
  envelopeLane: 104,
  /** Board viewport the design is drawn at. */
  boardWidth: 1440,
  boardHeight: 900,
} as const;

/** DESIGN.md: monospace for all numbers — they are read as values, not prose. */
export const font = {
  sans: "'IBM Plex Sans', system-ui, sans-serif",
  mono: "'IBM Plex Mono', ui-monospace, monospace",
  serif: "'Instrument Serif', Georgia, serif",
} as const;

export const tokens = { color, wash, metric, font } as const;

export type ColorToken = keyof typeof color;
export type MetricToken = keyof typeof metric;

/** camelCase -> kebab-case, for custom property names. */
function kebab(name: string): string {
  return name.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`);
}

/**
 * Emits every token as a CSS custom property under `:root`.
 *
 * Injected once at boot by the app entry point; all stylesheets then reference
 * `var(--df-brass)` / `var(--df-shelf)` and never a literal value. Colours keep
 * their token name (`--df-brass`); metrics gain a `px` unit (`--df-shelf: 216px`).
 */
export function tokensToCssText(): string {
  const lines: string[] = [];
  for (const [key, value] of Object.entries(color)) lines.push(`--df-${kebab(key)}: ${value};`);
  for (const [key, value] of Object.entries(wash)) lines.push(`--df-wash-${kebab(key)}: ${value};`);
  for (const [key, value] of Object.entries(metric)) lines.push(`--df-${kebab(key)}: ${value}px;`);
  for (const [key, value] of Object.entries(font)) lines.push(`--df-font-${kebab(key)}: ${value};`);
  return `:root {\n  ${lines.join('\n  ')}\n}`;
}
