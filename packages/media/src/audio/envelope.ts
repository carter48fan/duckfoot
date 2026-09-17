import { dbToAmplitude } from '@duckfoot/core';
import type { EnvelopeInterpolation, EnvelopeNode, GainEnvelope, Selection } from '@duckfoot/core';

/**
 * The gain envelope over the selection.
 *
 * `nodes` is the canonical state. The Inspector's "Ramp in 420 ms" and "Ramp out
 * 300 ms" are derived from node spacing, never stored — see `rampInMs` in
 * @duckfoot/core, and note the board itself proves the point by showing an in-point
 * of 0:42.118 and a second node at 0:48.940.
 *
 * The three interpolations are genuinely ambiguous words, so they are pinned here:
 *   'lin'     — linear in amplitude (a straight line on a waveform display)
 *   'log'     — linear in decibels (what people usually mean by a "smooth" fade)
 *   's-curve' — smoothstep in decibels (eased at both ends)
 */
function interpolate(t: number, interpolation: EnvelopeInterpolation): number {
  switch (interpolation) {
    case 's-curve':
      return t * t * (3 - 2 * t);
    case 'lin':
    case 'log':
    default:
      return t;
  }
}

export function evaluateEnvelopeDb(envelope: GainEnvelope, timeSeconds: number): number {
  const nodes = envelope.nodes;
  if (nodes.length === 0) return 0;
  if (timeSeconds <= nodes[0].timeSeconds) return nodes[0].gainDb;
  const last = nodes[nodes.length - 1];
  if (timeSeconds >= last.timeSeconds) return last.gainDb;

  for (let i = 0; i < nodes.length - 1; i++) {
    const a = nodes[i];
    const b = nodes[i + 1];
    if (timeSeconds > b.timeSeconds) continue;
    const span = b.timeSeconds - a.timeSeconds;
    if (span <= 0) return b.gainDb;
    const t = interpolate((timeSeconds - a.timeSeconds) / span, envelope.interpolation);

    if (envelope.interpolation === 'lin') {
      // Linear in amplitude, so convert, blend, convert back.
      const ampA = dbToAmplitude(a.gainDb);
      const ampB = dbToAmplitude(b.gainDb);
      const amp = ampA + (ampB - ampA) * t;
      return amp <= 0 ? -120 : 20 * Math.log10(amp);
    }
    return a.gainDb + (b.gainDb - a.gainDb) * t;
  }
  return last.gainDb;
}

export function evaluateEnvelopeGain(envelope: GainEnvelope, timeSeconds: number): number {
  return dbToAmplitude(evaluateEnvelopeDb(envelope, timeSeconds));
}

/**
 * The four-node duck the board shows: flat, down to the region gain, hold, back up.
 *
 * Only ever used to construct an initial shape — once the nodes exist the user drags
 * them freely and this is not consulted again.
 */
export function buildDuckEnvelope(
  selection: Selection,
  regionGainDb: number,
  rampInMs: number,
  rampOutMs: number,
  interpolation: EnvelopeInterpolation = 'lin'
): GainEnvelope {
  const { inSeconds, outSeconds } = selection;
  const rampIn = rampInMs / 1000;
  const rampOut = rampOutMs / 1000;
  const nodes: EnvelopeNode[] = [
    { timeSeconds: inSeconds, gainDb: 0 },
    { timeSeconds: Math.min(inSeconds + rampIn, outSeconds), gainDb: regionGainDb },
    { timeSeconds: Math.max(outSeconds - rampOut, inSeconds), gainDb: regionGainDb },
    { timeSeconds: outSeconds, gainDb: 0 },
  ].sort((a, b) => a.timeSeconds - b.timeSeconds);
  return { nodes, interpolation };
}

/**
 * Applies an envelope to a buffer, returning new data.
 *
 * Never mutates its input — DESIGN.md invariant 5. `applyFade` is now a thin wrapper
 * over this, so there is one envelope engine rather than two fade implementations
 * that drift apart.
 */
export function applyGainEnvelope(
  context: BaseAudioContext,
  buffer: AudioBuffer,
  envelope: GainEnvelope,
  bufferStartSeconds = 0
): AudioBuffer {
  const out = context.createBuffer(buffer.numberOfChannels, buffer.length, buffer.sampleRate);
  const invRate = 1 / buffer.sampleRate;

  for (let c = 0; c < buffer.numberOfChannels; c++) {
    const src = buffer.getChannelData(c);
    const dst = out.getChannelData(c);
    for (let i = 0; i < src.length; i++) {
      dst[i] = src[i] * evaluateEnvelopeGain(envelope, bufferStartSeconds + i * invRate);
    }
  }
  return out;
}
