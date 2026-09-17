import type { Command } from './commands';
import { coalesceKeyOf, describeCommand } from './commands';
import type { DuckfootDocument } from './types';
import { createSuiteDocument } from './types';
import { createPhotoDocument, createRawStack, RAW_MODULE_META, RAW_MODULE_ORDER } from './photo';
import type { RawModuleId, RawModuleInstance } from './photo';
import { createAudioDocument, MAX_PHASE_OFFSET_SECONDS } from './audio';
import type { Lane } from './lane';
import { clamp } from '../utils/math';

/**
 * Undo is snapshot-based, not inverse-command-based.
 *
 * The usual objection to snapshots is size, and the usual answer is inverse
 * commands — but inverse commands desync the first time somebody writes one that
 * isn't a true inverse, and that class of bug is invisible until a user hits it.
 *
 * Snapshots are viable here precisely because of the rule that documents hold ids
 * and numbers and never buffers or bitmaps. A whole DuckfootDocument is a few
 * kilobytes of JSON. Snapshots cannot desync, and they make 2a's HISTORY panel
 * time-travel (`jumpTo`) and its SNAPSHOTS panel almost free.
 */
export interface HistoryEntry {
  /** Rendered in 2a's HISTORY list. */
  label: string;
  /** null for the initial entry, which represents the unedited document. */
  command: Command | null;
  at: number;
  coalesceKey: string | null;
  /** The document *after* this command. */
  doc: DuckfootDocument;
}

export interface AppState {
  history: HistoryEntry[];
  /** Index into `history` of the current document. */
  index: number;
}

export interface UndoOptions {
  /** Entries retained before the tail is dropped. */
  limit?: number;
  /** Commands sharing a coalesce key within this many ms collapse into one entry. */
  coalesceMs?: number;
}

const DEFAULT_LIMIT = 200;
const DEFAULT_COALESCE_MS = 500;

export function createDocument(): DuckfootDocument {
  return {
    suite: createSuiteDocument(),
    photo: createPhotoDocument(),
    audio: createAudioDocument(),
  };
}

export function createAppState(doc: DuckfootDocument = createDocument()): AppState {
  return {
    history: [{ label: 'original', command: null, at: Date.now(), coalesceKey: null, doc }],
    index: 0,
  };
}

export function currentDocument(state: AppState): DuckfootDocument {
  return state.history[state.index].doc;
}

export function canUndo(state: AppState): boolean {
  return state.index > 0;
}

export function canRedo(state: AppState): boolean {
  return state.index < state.history.length - 1;
}

/** Entries at or before the cursor, newest first — the order 2a's HISTORY renders. */
export function visibleHistory(state: AppState): HistoryEntry[] {
  return state.history.slice(0, state.index + 1).reverse();
}

export function execute(state: AppState, command: Command, options: UndoOptions = {}): AppState {
  const limit = options.limit ?? DEFAULT_LIMIT;
  const coalesceMs = options.coalesceMs ?? DEFAULT_COALESCE_MS;

  const base = currentDocument(state);
  const next = applyCommand(base, command);
  // A command that changed nothing should not occupy a history slot.
  if (next === base) return state;

  const key = coalesceKeyOf(command);
  const now = Date.now();
  const head = state.history[state.index];

  // Redo branch is discarded the moment a new command lands on top of it.
  const truncated = state.history.slice(0, state.index + 1);

  const shouldCoalesce =
    key !== null && state.index > 0 && head.coalesceKey === key && now - head.at <= coalesceMs;

  if (shouldCoalesce) {
    // Replace the head in place: one drag, one entry. `at` advances so a continuous
    // drag keeps coalescing rather than expiring mid-gesture.
    const merged: HistoryEntry = { ...head, at: now, doc: next, command };
    const history = [...truncated.slice(0, -1), merged];
    return { history, index: history.length - 1 };
  }

  const entry: HistoryEntry = {
    label: describeCommand(command),
    command,
    at: now,
    coalesceKey: key,
    doc: next,
  };

  let history = [...truncated, entry];
  let index = history.length - 1;
  if (history.length > limit) {
    const overflow = history.length - limit;
    history = history.slice(overflow);
    index -= overflow;
  }
  return { history, index };
}

export function undo(state: AppState): AppState {
  if (!canUndo(state)) return state;
  return { ...state, index: state.index - 1 };
}

export function redo(state: AppState): AppState {
  if (!canRedo(state)) return state;
  return { ...state, index: state.index + 1 };
}

/** Time travel, for clicking a row in 2a's HISTORY panel. */
export function jumpTo(state: AppState, index: number): AppState {
  if (index < 0 || index >= state.history.length) return state;
  return { ...state, index };
}

// ---------------------------------------------------------------------------
// The reducer. Pure: no history, no side effects, returns the same object when a
// command is a no-op so `execute` can skip the entry.
// ---------------------------------------------------------------------------

export function applyCommand(doc: DuckfootDocument, command: Command): DuckfootDocument {
  switch (command.type) {
    // --- Shelf --------------------------------------------------------------
    case 'suite/addAssets': {
      if (command.assets.length === 0) return doc;
      return { ...doc, suite: { ...doc.suite, assets: [...doc.suite.assets, ...command.assets] } };
    }
    case 'suite/removeAsset': {
      const assets = doc.suite.assets.filter((a) => a.id !== command.assetId);
      if (assets.length === doc.suite.assets.length) return doc;
      return { ...doc, suite: { ...doc.suite, assets } };
    }
    case 'suite/renameAsset': {
      const assets = doc.suite.assets.map((a) =>
        a.id === command.assetId ? { ...a, name: command.name } : a
      );
      return { ...doc, suite: { ...doc.suite, assets } };
    }

    // --- Photo --------------------------------------------------------------
    case 'photo/openAsset':
      return {
        ...doc,
        photo: {
          ...doc.photo,
          assetId: command.assetId,
          kind: command.kind,
          stack: createRawStack(),
        },
      };

    case 'photo/setModuleParam': {
      const id = command.moduleId;
      const instance = doc.photo.stack[id] as RawModuleInstance;
      const params = { ...instance.params, ...command.patch };
      const stack = {
        ...doc.photo.stack,
        [id]: { ...instance, params },
      } as typeof doc.photo.stack;
      return { ...doc, photo: { ...doc.photo, stack } };
    }

    case 'photo/toggleModule': {
      const id = command.moduleId;
      const instance = doc.photo.stack[id] as RawModuleInstance;
      if (instance.enabled === command.enabled) return doc;
      const stack = {
        ...doc.photo.stack,
        [id]: { ...instance, enabled: command.enabled },
      } as typeof doc.photo.stack;
      return { ...doc, photo: { ...doc.photo, stack } };
    }

    case 'photo/resetAllModules':
      return { ...doc, photo: { ...doc.photo, stack: createRawStack() } };

    case 'photo/pasteStack':
    case 'photo/restoreSnapshot':
      return { ...doc, photo: { ...doc.photo, stack: structuredClone(command.stack) } };

    case 'photo/setAdjustments':
      return {
        ...doc,
        photo: { ...doc.photo, adjustments: { ...doc.photo.adjustments, ...command.patch } },
      };

    case 'photo/setWatermark':
      if (doc.photo.watermark === command.text) return doc;
      return { ...doc, photo: { ...doc.photo, watermark: command.text } };

    // --- Audio --------------------------------------------------------------
    case 'audio/addLane': {
      if (doc.audio.lanes[command.laneId]) return doc;
      const lane: Lane = {
        id: command.laneId,
        name: command.name,
        assetId: command.assetId,
        gainDb: 0,
        muted: false,
        soloed: false,
        phase: { polarityInverted: false, offsetSeconds: 0, rotationDegrees: 0 },
        // The first lane in becomes the phase reference; the board shows drums
        // holding that role.
        isPhaseReference: doc.audio.laneOrder.length === 0,
      };
      return {
        ...doc,
        audio: {
          ...doc.audio,
          laneOrder: [...doc.audio.laneOrder, command.laneId],
          lanes: { ...doc.audio.lanes, [command.laneId]: lane },
        },
      };
    }

    case 'audio/removeLane': {
      if (!doc.audio.lanes[command.laneId]) return doc;
      const lanes = { ...doc.audio.lanes };
      delete lanes[command.laneId];
      const laneOrder = doc.audio.laneOrder.filter((id) => id !== command.laneId);
      // Never leave a stack without a reference lane.
      if (laneOrder.length > 0 && !laneOrder.some((id) => lanes[id].isPhaseReference)) {
        lanes[laneOrder[0]] = { ...lanes[laneOrder[0]], isPhaseReference: true };
      }
      return { ...doc, audio: { ...doc.audio, laneOrder, lanes } };
    }

    case 'audio/setLaneGain':
      return patchLane(doc, command.laneId, (lane) =>
        lane.gainDb === command.gainDb ? lane : { ...lane, gainDb: command.gainDb }
      );

    case 'audio/setLaneMuted':
      return patchLane(doc, command.laneId, (lane) =>
        lane.muted === command.muted ? lane : { ...lane, muted: command.muted }
      );

    case 'audio/setLaneSoloed':
      return patchLane(doc, command.laneId, (lane) =>
        lane.soloed === command.soloed ? lane : { ...lane, soloed: command.soloed }
      );

    case 'audio/setLanePhase':
      return patchLane(doc, command.laneId, (lane) => {
        const phase = { ...lane.phase, ...command.patch };
        // The bound lives here, not in the slider. A limit that only exists in one
        // UI control is a limit the next UI control forgets, and an unbounded offset
        // is the timeline DESIGN.md refuses.
        phase.offsetSeconds = clamp(
          phase.offsetSeconds,
          -MAX_PHASE_OFFSET_SECONDS,
          MAX_PHASE_OFFSET_SECONDS
        );
        phase.rotationDegrees = clamp(phase.rotationDegrees, -180, 180);
        return { ...lane, phase };
      });

    case 'audio/setReferenceLane': {
      if (!doc.audio.lanes[command.laneId]) return doc;
      const lanes: Record<string, Lane> = {};
      for (const [id, lane] of Object.entries(doc.audio.lanes)) {
        lanes[id] = { ...lane, isPhaseReference: id === command.laneId };
      }
      return { ...doc, audio: { ...doc.audio, lanes } };
    }

    case 'audio/setSelection': {
      const inSeconds = Math.min(command.inSeconds, command.outSeconds);
      const outSeconds = Math.max(command.inSeconds, command.outSeconds);
      const current = doc.audio.selection;
      if (current && current.inSeconds === inSeconds && current.outSeconds === outSeconds) return doc;
      return { ...doc, audio: { ...doc.audio, selection: { inSeconds, outSeconds } } };
    }

    case 'audio/clearSelection':
      if (!doc.audio.selection) return doc;
      return { ...doc, audio: { ...doc.audio, selection: null } };

    case 'audio/setEnvelopeNodes': {
      const nodes = [...command.nodes].sort((a, b) => a.timeSeconds - b.timeSeconds);
      const envelope = doc.audio.envelope
        ? { ...doc.audio.envelope, nodes }
        : { nodes, interpolation: 'lin' as const };
      return { ...doc, audio: { ...doc.audio, envelope } };
    }

    case 'audio/setEnvelopeInterpolation': {
      if (!doc.audio.envelope) return doc;
      return {
        ...doc,
        audio: {
          ...doc.audio,
          envelope: { ...doc.audio.envelope, interpolation: command.interpolation },
        },
      };
    }

    case 'audio/clearEnvelope':
      if (!doc.audio.envelope) return doc;
      return { ...doc, audio: { ...doc.audio, envelope: null } };

    case 'audio/deleteRegion': {
      const sel = doc.audio.selection;
      if (!sel) return doc;
      return {
        ...doc,
        audio: {
          ...doc.audio,
          edits: [...doc.audio.edits, { kind: 'delete', ...sel }],
          selection: null,
        },
      };
    }

    case 'audio/trimToSelection': {
      const sel = doc.audio.selection;
      if (!sel) return doc;
      return {
        ...doc,
        audio: { ...doc.audio, edits: [...doc.audio.edits, { kind: 'trim', ...sel }] },
      };
    }

    case 'audio/normalizeSelection': {
      const sel = doc.audio.selection;
      if (!sel) return doc;
      return {
        ...doc,
        audio: {
          ...doc.audio,
          edits: [
            ...doc.audio.edits,
            { kind: 'normalize', ...sel, targetPeakDb: command.targetPeakDb },
          ],
        },
      };
    }
  }
}

function patchLane(
  doc: DuckfootDocument,
  laneId: string,
  update: (lane: Lane) => Lane
): DuckfootDocument {
  const lane = doc.audio.lanes[laneId];
  if (!lane) return doc;
  const next = update(lane);
  if (next === lane) return doc;
  return { ...doc, audio: { ...doc.audio, lanes: { ...doc.audio.lanes, [laneId]: next } } };
}

/** Modules whose implementation is missing, for badging rows in the Inspector. */
export function stubbedModules(): RawModuleId[] {
  return RAW_MODULE_ORDER.filter((id) => RAW_MODULE_META[id].status === 'stub');
}
