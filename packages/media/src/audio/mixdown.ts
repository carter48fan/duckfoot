import { dbToAmplitude } from '@duckfoot/core';
import type { GainEnvelope, LanePhase } from '@duckfoot/core';
import { evaluateEnvelopeGain } from './envelope';

export interface LaneRenderSpec {
  buffer: AudioBuffer;
  gainDb: number;
  muted: boolean;
  phase: LanePhase;
}

export interface LaneGraph {
  source: AudioBufferSourceNode;
  /** Connect this to the destination, or to a master gain. */
  output: AudioNode;
}

/**
 * Builds the node chain for one lane.
 *
 * DESIGN.md invariant 2 again, in its audio form: live monitoring and the exported
 * mixdown construct their graph through this single function, so they cannot drift.
 * Anything this cannot express (phase rotation, which has no Web Audio equivalent)
 * is therefore absent from both paths rather than one.
 *
 * Polarity inversion is a gain of −1, which is exact and costs nothing. The time
 * offset is a DelayNode; it can only delay, so the whole stack is pre-rolled by
 * `commonDelaySeconds` and each lane delays relative to that. That is what lets a
 * lane sit *earlier* than the reference without any lane sliding on screen.
 */
export function buildLaneGraph(
  context: BaseAudioContext,
  spec: LaneRenderSpec,
  commonDelaySeconds: number
): LaneGraph {
  const source = context.createBufferSource();
  source.buffer = spec.buffer;

  const polarity = context.createGain();
  polarity.gain.value = spec.phase.polarityInverted ? -1 : 1;

  const delay = context.createDelay(Math.max(1, commonDelaySeconds * 2));
  delay.delayTime.value = Math.max(0, commonDelaySeconds + spec.phase.offsetSeconds);

  const gain = context.createGain();
  gain.gain.value = spec.muted ? 0 : dbToAmplitude(spec.gainDb);

  source.connect(polarity);
  polarity.connect(delay);
  delay.connect(gain);

  return { source, output: gain };
}

export interface MixdownOptions {
  startSeconds: number;
  endSeconds: number;
  envelope?: GainEnvelope | null;
  sampleRate?: number;
}

export interface MixdownResult {
  buffer: AudioBuffer;
  /** Operations requested by the document that no implementation could honour. */
  stubbedOps: string[];
}

/**
 * Renders the active lanes to a single buffer.
 *
 * Reports rather than throws when a lane asks for an operation that does not exist
 * yet, for the same reason the RAW pipeline does: one missing operation should not
 * take down a mixdown that is otherwise correct, but the caller must be able to
 * tell the user that the export is not what the document describes.
 */
export async function mixdownLanes(
  lanes: LaneRenderSpec[],
  options: MixdownOptions
): Promise<MixdownResult> {
  const audible = lanes.filter((lane) => !lane.muted);
  const stubbedOps: string[] = [];
  if (lanes.some((lane) => lane.phase.rotationDegrees !== 0)) {
    stubbedOps.push('rotatePhase');
  }

  const first = lanes[0]?.buffer;
  const sampleRate = options.sampleRate ?? first?.sampleRate ?? 48000;
  const channels = Math.max(1, ...lanes.map((lane) => lane.buffer.numberOfChannels));
  const duration = Math.max(0, options.endSeconds - options.startSeconds);
  const frames = Math.max(1, Math.ceil(duration * sampleRate));

  // Pre-roll, so a negative lane offset is expressible as a smaller positive delay.
  const maxAdvance = Math.max(0, ...lanes.map((lane) => -lane.phase.offsetSeconds));
  const context = new OfflineAudioContext(channels, frames + Math.ceil(maxAdvance * sampleRate), sampleRate);

  const master = context.createGain();
  master.connect(context.destination);

  for (const lane of audible) {
    const { source, output } = buildLaneGraph(context, lane, maxAdvance);
    output.connect(master);
    source.start(0, options.startSeconds, duration + maxAdvance);
  }

  if (options.envelope) {
    // Sampled at 200 Hz — well above any envelope shape a person can draw, and far
    // cheaper than a per-sample automation curve.
    const steps = Math.max(2, Math.ceil(duration * 200));
    const curve = new Float32Array(steps);
    for (let i = 0; i < steps; i++) {
      const t = options.startSeconds + (i / (steps - 1)) * duration;
      curve[i] = evaluateEnvelopeGain(options.envelope, t);
    }
    master.gain.setValueCurveAtTime(curve, maxAdvance, duration);
  }

  const rendered = await context.startRendering();

  if (maxAdvance === 0) return { buffer: rendered, stubbedOps };

  // Drop the pre-roll so the result starts exactly at the selection's in-point.
  const offset = Math.ceil(maxAdvance * sampleRate);
  const trimmed = new OfflineAudioContext(channels, frames, sampleRate).createBuffer(
    channels,
    frames,
    sampleRate
  );
  for (let c = 0; c < channels; c++) {
    trimmed.getChannelData(c).set(rendered.getChannelData(c).subarray(offset, offset + frames));
  }
  return { buffer: trimmed, stubbedOps };
}
