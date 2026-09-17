import type { LanePhase } from './audio';

/**
 * A lane. Never a track.
 *
 * DESIGN.md is explicit that the word choice is load-bearing: "Different word,
 * different mental model, and it stops the distinction eroding the first time
 * someone reasonably asks 'can I just nudge this one a bit.'" Barrel 3 has tracks
 * — see `@duckfoot/tool-video` — and that is correct there. Barrel 2 has lanes.
 *
 * Note what a lane does NOT hold: an AudioBuffer. Documents carry ids and numbers
 * only, which is what makes snapshot undo cheap and what structurally prevents the
 * in-place mutation DESIGN.md invariant 5 forbids — you cannot mutate a source
 * buffer that the document never held a reference to.
 */
export interface Lane {
  id: string;
  /** 'drums' | 'bass' | 'vox' */
  name: string;
  /** Resolves against the Shelf. Never the decoded audio itself. */
  assetId: string;
  gainDb: number;
  muted: boolean;
  soloed: boolean;
  phase: LanePhase;
  /** The lane others align against. Exactly one lane in a stack has this. */
  isPhaseReference: boolean;
}

export const DEFAULT_LANE_GAIN_DB = 0;

export function createLane(id: string, name: string, assetId: string): Lane {
  return {
    id,
    name,
    assetId,
    gainDb: DEFAULT_LANE_GAIN_DB,
    muted: false,
    soloed: false,
    phase: { polarityInverted: false, offsetSeconds: 0, rotationDegrees: 0 },
    isPhaseReference: false,
  };
}

/**
 * Lanes that should actually be heard, honouring solo-over-mute.
 * Used by both the live playback graph and the mixdown, so they cannot disagree.
 */
export function activeLanes(lanes: readonly Lane[]): Lane[] {
  const soloed = lanes.filter((lane) => lane.soloed);
  if (soloed.length > 0) return soloed;
  return lanes.filter((lane) => !lane.muted);
}
