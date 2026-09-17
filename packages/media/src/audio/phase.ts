import { notImplemented } from '@duckfoot/core';
import type { LanePhase } from '@duckfoot/core';

/**
 * Per-lane phase alignment.
 *
 * DESIGN.md's amended position: an offset here is an alignment correction, not a
 * position. It is bounded (±50 ms, enforced in the command), edited numerically,
 * and never drawn as a horizontal shift of a waveform. That is the distinction
 * that keeps the stem stack from becoming a timeline.
 */

/** Flips the sign of every sample. Returns new data. */
export function invertPolarity(context: BaseAudioContext, buffer: AudioBuffer): AudioBuffer {
  const out = context.createBuffer(buffer.numberOfChannels, buffer.length, buffer.sampleRate);
  for (let c = 0; c < buffer.numberOfChannels; c++) {
    const src = buffer.getChannelData(c);
    const dst = out.getChannelData(c);
    for (let i = 0; i < src.length; i++) dst[i] = -src[i];
  }
  return out;
}

/**
 * Whole-sample shift. Positive delays, negative advances.
 *
 * Length is preserved: samples shifted past either end are dropped and the vacated
 * region is silence. A lane must stay the same length as its neighbours or the
 * shared selection stops meaning the same thing on every lane.
 */
export function applyIntegerDelay(
  context: BaseAudioContext,
  buffer: AudioBuffer,
  samples: number
): AudioBuffer {
  const out = context.createBuffer(buffer.numberOfChannels, buffer.length, buffer.sampleRate);
  if (samples === 0) {
    for (let c = 0; c < buffer.numberOfChannels; c++) {
      out.getChannelData(c).set(buffer.getChannelData(c));
    }
    return out;
  }

  for (let c = 0; c < buffer.numberOfChannels; c++) {
    const src = buffer.getChannelData(c);
    const dst = out.getChannelData(c);
    for (let i = 0; i < src.length; i++) {
      const j = i - samples;
      dst[i] = j >= 0 && j < src.length ? src[j] : 0;
    }
  }
  return out;
}

/**
 * @stub Sub-sample delay via windowed-sinc interpolation.
 *
 * `applyIntegerDelay` rounds to the nearest sample, which at 48 kHz is ~20.8 µs of
 * residual error — audible as comb filtering when summing two close-miked sources,
 * which is the exact job this control exists for. A 32-tap Kaiser-windowed sinc
 * would resolve the board's −68.16 samples properly. Until then the UI rounds and
 * says so rather than claiming a precision it does not have.
 */
export function applyFractionalDelay(
  context: BaseAudioContext,
  buffer: AudioBuffer,
  seconds: number
): AudioBuffer {
  void context;
  void buffer;
  void seconds;
  return notImplemented('applyFractionalDelay', 'needs a windowed-sinc interpolator');
}

/**
 * @stub Broadband phase rotation.
 *
 * A correct all-pass rotation needs a Hilbert transform, which needs an FFT, and
 * there is no FFT in the tree. Note this has no Web Audio equivalent either, so it
 * is stubbed consistently on both the live-playback and mixdown paths — a stub that
 * applied on one path and not the other would make export disagree with preview.
 */
export function rotatePhase(
  context: BaseAudioContext,
  buffer: AudioBuffer,
  degrees: number
): AudioBuffer {
  void context;
  void buffer;
  void degrees;
  return notImplemented('rotatePhase', 'needs an FFT for the Hilbert transform');
}

/** Whole samples for a given offset, which is what `applyIntegerDelay` can honour. */
export function offsetInSamples(phase: LanePhase, sampleRate: number): number {
  return Math.round(phase.offsetSeconds * sampleRate);
}
