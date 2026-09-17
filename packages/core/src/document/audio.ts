import type { Lane } from './lane';

/**
 * Per-lane phase alignment.
 *
 * This is the capability DESIGN.md originally refused ("No per-lane horizontal
 * offset"), and the reasoning for the refusal is worth keeping in view: "The moment
 * one lane can slide against another you must visualise the offset, and that
 * visualisation *is* a timeline."
 *
 * The line that keeps the product thesis intact is that the offset is never
 * visualised. Selection, playhead and ruler are all in stack time; every lane's
 * waveform is drawn at the same origin. The offset appears as a numeric badge and
 * is edited numerically in the Inspector. It has no horizontal drag handle, and it
 * is bounded — an alignment correction, not a position.
 */
export interface LanePhase {
  /** The ø button. A polarity flip, not a delay. */
  polarityInverted: boolean;
  /**
   * Alignment nudge in seconds, bounded to ±MAX_PHASE_OFFSET_SECONDS.
   *
   * This is the single source of truth for the offset. The board's "−68 smp" is
   * derived (`offsetSeconds * sampleRate`) and is never stored — two
   * representations of one quantity is how a readout and a render come to disagree.
   */
  offsetSeconds: number;
  /** All-pass phase rotation in degrees, −180..+180. */
  rotationDegrees: number;
}

/**
 * ±50 ms. Beyond this you are positioning rather than aligning, and positioning is
 * Barrel 3's job. Enforced in the command, not in the slider — a bound that only
 * exists in a UI control is a bound that a second UI control forgets.
 */
export const MAX_PHASE_OFFSET_SECONDS = 0.05;

export const DEFAULT_LANE_PHASE: LanePhase = {
  polarityInverted: false,
  offsetSeconds: 0,
  rotationDegrees: 0,
};

/** One shared in/out across the whole stack. Never per-lane. */
export interface Selection {
  inSeconds: number;
  outSeconds: number;
}

export type EnvelopeInterpolation = 'lin' | 'log' | 's-curve';

export interface EnvelopeNode {
  /** Absolute stack time, seconds. */
  timeSeconds: number;
  gainDb: number;
}

/**
 * The gain envelope over the selection.
 *
 * DESIGN.md's amended refusal: ONE envelope, gain only, applied to active lanes
 * together. Not per-lane, not per-parameter, not per-effect. That constraint is
 * what keeps this from being the first curve in an automation system.
 *
 * `nodes` is canonical. The Inspector's "Ramp in 420 ms" / "Ramp out 300 ms" are
 * derived readouts (the gap between the first two and last two nodes), editable by
 * typing but never stored — the board itself proves they must be derived, showing
 * in at 0:42.118 and the second node at 0:48.940, which is 6.8 s apart, not 420 ms.
 */
export interface GainEnvelope {
  /** Sorted by time, at least two nodes. */
  nodes: EnvelopeNode[];
  interpolation: EnvelopeInterpolation;
}


/**
 * A destructive-looking edit, recorded rather than applied.
 *
 * The document never holds audio, so "delete this region" cannot mean splicing a
 * buffer — it means appending an entry here, which the render and the mixdown both
 * honour. That keeps invariant 5 ("never mutate a source buffer; operations return
 * new data") true by construction, makes "Revert to original" a matter of clearing
 * a list, and keeps the document small enough for snapshot undo.
 *
 * Applied in order, each against the result of the previous.
 */
export type RegionEdit =
  | { kind: 'delete'; inSeconds: number; outSeconds: number }
  | { kind: 'trim'; inSeconds: number; outSeconds: number }
  | { kind: 'normalize'; inSeconds: number; outSeconds: number; targetPeakDb: number };

export interface AudioDocument {
  laneOrder: string[];
  lanes: Record<string, Lane>;
  selection: Selection | null;
  envelope: GainEnvelope | null;
  /** Non-destructive edit list. See RegionEdit. */
  edits: RegionEdit[];
  sampleRate: number;
  durationSeconds: number;
}

export function createAudioDocument(): AudioDocument {
  return {
    laneOrder: [],
    lanes: {},
    selection: null,
    envelope: null,
    edits: [],
    sampleRate: 48000,
    durationSeconds: 0,
  };
}

export function orderedLanes(doc: AudioDocument): Lane[] {
  return doc.laneOrder.map((id) => doc.lanes[id]).filter((lane): lane is Lane => Boolean(lane));
}

/** Derived readout, never stored. See `GainEnvelope.nodes`. */
export function rampInMs(envelope: GainEnvelope): number {
  if (envelope.nodes.length < 2) return 0;
  return (envelope.nodes[1].timeSeconds - envelope.nodes[0].timeSeconds) * 1000;
}

/** Derived readout, never stored. See `GainEnvelope.nodes`. */
export function rampOutMs(envelope: GainEnvelope): number {
  const n = envelope.nodes.length;
  if (n < 2) return 0;
  return (envelope.nodes[n - 1].timeSeconds - envelope.nodes[n - 2].timeSeconds) * 1000;
}
