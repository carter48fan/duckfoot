import { notImplemented } from '@duckfoot/core';

export interface CorrelationResult {
  lagSamples: number;
  /** Pearson coefficient at the best lag, −1..+1. */
  coefficient: number;
}

/**
 * @stub Cross-correlation between two lanes over the selection.
 *
 * This is what would produce the board's "+0.76 in phase". It is deliberately NOT
 * faked: "Measured across the selection on active lanes" is a factual claim printed
 * in the UI, and a plausible-looking number there would be exactly the kind of lie
 * about the media that DESIGN.md's look-and-feel section exists to prevent. The
 * correlation meter renders an em dash until this is real.
 *
 * The implementation is not large — mono-sum, decimate 8:1, slide over ±50 ms of
 * lag, take the peak — roughly forty lines. It is stubbed because this turn is
 * scoped to the boards' UI, not because it is hard.
 */
export function crossCorrelate(
  a: Float32Array,
  b: Float32Array,
  maxLagSamples: number
): CorrelationResult {
  void a;
  void b;
  void maxLagSamples;
  return notImplemented('crossCorrelate', 'deferred with the rest of the DSP layer');
}

export interface LaneAlignment {
  offsetSeconds: number;
  correlation: number;
  /** A strong negative peak means the lanes are out of polarity, not out of time. */
  suggestPolarityInvert: boolean;
}

/**
 * @stub Auto-align a lane against the phase reference.
 *
 * Returns a proposal; the UI turns it into a command so the move lands on the undo
 * stack like any other edit. Never mutates.
 */
export function autoAlignToReference(
  reference: AudioBuffer,
  lane: AudioBuffer,
  selectionStartSeconds: number,
  selectionEndSeconds: number
): LaneAlignment {
  void reference;
  void lane;
  void selectionStartSeconds;
  void selectionEndSeconds;
  return notImplemented('autoAlignToReference', 'depends on crossCorrelate');
}
