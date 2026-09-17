import type { AssetItem } from '../types';
import type { ImageAdjustments, RawModuleId, RawModuleParamMap, RawStack } from './photo';
import { RAW_MODULE_META } from './photo';
import type { EnvelopeInterpolation, EnvelopeNode, LanePhase } from './audio';

/**
 * Every mutation is a command.
 *
 * DESIGN.md invariant 3: "Every mutation is a command on an undo stack. No
 * exceptions, no 'just this one little toggle.' Because undo is what makes people
 * trust an editor, and it cannot be retrofitted."
 *
 * Commands are a plain serialisable union rather than objects carrying an `apply`
 * closure. That keeps them loggable, testable and replayable, and it keeps the
 * reducer in one readable place instead of scattered across fourteen modules.
 *
 * What is NOT a command: view state. Module expand/collapse, the A/B divider, zoom,
 * which tab is open, whether a panel is collapsed. None of these touch the media,
 * so none belong on the stack — Ctrl+Z must never undo a panel toggle. This is the
 * first thing a contributor gets wrong; it is written here and in `undo.ts`.
 */
export type Command =
  // --- Shelf ----------------------------------------------------------------
  | { type: 'suite/addAssets'; assets: AssetItem[] }
  | { type: 'suite/removeAsset'; assetId: string }
  | { type: 'suite/renameAsset'; assetId: string; name: string }
  // --- Photo ----------------------------------------------------------------
  | { type: 'photo/openAsset'; assetId: string; kind: 'raster' | 'raw' }
  | {
      type: 'photo/setModuleParam';
      moduleId: RawModuleId;
      patch: Partial<RawModuleParamMap[RawModuleId]>;
    }
  | { type: 'photo/toggleModule'; moduleId: RawModuleId; enabled: boolean }
  | { type: 'photo/resetAllModules' }
  | { type: 'photo/pasteStack'; stack: RawStack; targetAssetIds: string[] }
  | { type: 'photo/restoreSnapshot'; name: string; stack: RawStack }
  | { type: 'photo/setAdjustments'; patch: Partial<ImageAdjustments> }
  | { type: 'photo/setWatermark'; text: string }
  // --- Audio ----------------------------------------------------------------
  | { type: 'audio/addLane'; laneId: string; name: string; assetId: string }
  | { type: 'audio/removeLane'; laneId: string }
  | { type: 'audio/setLaneGain'; laneId: string; gainDb: number }
  | { type: 'audio/setLaneMuted'; laneId: string; muted: boolean }
  | { type: 'audio/setLaneSoloed'; laneId: string; soloed: boolean }
  | { type: 'audio/setLanePhase'; laneId: string; patch: Partial<LanePhase> }
  | { type: 'audio/setReferenceLane'; laneId: string }
  | { type: 'audio/setSelection'; inSeconds: number; outSeconds: number }
  | { type: 'audio/clearSelection' }
  | { type: 'audio/setEnvelopeNodes'; nodes: EnvelopeNode[] }
  | { type: 'audio/setEnvelopeInterpolation'; interpolation: EnvelopeInterpolation }
  | { type: 'audio/clearEnvelope' }
  | { type: 'audio/deleteRegion' }
  | { type: 'audio/normalizeSelection'; targetPeakDb: number }
  | { type: 'audio/trimToSelection' };

/**
 * The label shown in 2a's HISTORY panel and used for the undo tooltip.
 *
 * Module commands delegate to RAW_MODULE_META so the history row and the module row
 * can never disagree about a module's name.
 */
export function describeCommand(command: Command): string {
  switch (command.type) {
    case 'suite/addAssets':
      return command.assets.length === 1 ? 'import asset' : `import ${command.assets.length} assets`;
    case 'suite/removeAsset':
      return 'remove asset';
    case 'suite/renameAsset':
      return 'rename asset';
    case 'photo/openAsset':
      return 'open image';
    case 'photo/setModuleParam':
    case 'photo/toggleModule':
      return RAW_MODULE_META[command.moduleId].label;
    case 'photo/resetAllModules':
      return 'reset all modules';
    case 'photo/pasteStack':
      return command.targetAssetIds.length === 1
        ? 'paste stack'
        : `paste stack to ${command.targetAssetIds.length}`;
    case 'photo/restoreSnapshot':
      return `snapshot ${command.name}`;
    case 'photo/setAdjustments':
      return 'adjust';
    case 'photo/setWatermark':
      return 'watermark';
    case 'audio/addLane':
      return `add lane ${command.name}`;
    case 'audio/removeLane':
      return 'remove lane';
    case 'audio/setLaneGain':
      return 'gain';
    case 'audio/setLaneMuted':
      return command.muted ? 'mute' : 'unmute';
    case 'audio/setLaneSoloed':
      return command.soloed ? 'solo' : 'unsolo';
    case 'audio/setLanePhase':
      return 'phase';
    case 'audio/setReferenceLane':
      return 'phase reference';
    case 'audio/setSelection':
      return 'selection';
    case 'audio/clearSelection':
      return 'clear selection';
    case 'audio/setEnvelopeNodes':
      return 'gain envelope';
    case 'audio/setEnvelopeInterpolation':
      return 'envelope shape';
    case 'audio/clearEnvelope':
      return 'clear envelope';
    case 'audio/deleteRegion':
      return 'delete region';
    case 'audio/normalizeSelection':
      return 'normalize';
    case 'audio/trimToSelection':
      return 'trim to selection';
  }
}

/**
 * Commands sharing a coalesce key within the coalesce window collapse into one
 * history entry.
 *
 * This is not a nicety. A gain-slider drag emits roughly sixty commands; without
 * coalescing 2b's header reads "undo 340" instead of "undo 9" and 2a's HISTORY
 * panel is unusable. Returning `null` means "always a distinct entry".
 *
 * Coalescing lives here rather than in the UI on purpose — if the UI also debounced,
 * a drag would produce zero or duplicate entries depending on timing.
 */
export function coalesceKeyOf(command: Command): string | null {
  switch (command.type) {
    case 'photo/setModuleParam':
      return `photo/module/${command.moduleId}`;
    case 'photo/setAdjustments':
      return 'photo/adjustments';
    case 'photo/setWatermark':
      return 'photo/watermark';
    case 'audio/setLaneGain':
      return `audio/lane/${command.laneId}/gain`;
    case 'audio/setLanePhase':
      return `audio/lane/${command.laneId}/phase`;
    case 'audio/setSelection':
      return 'audio/selection';
    case 'audio/setEnvelopeNodes':
      return 'audio/envelope';
    default:
      return null;
  }
}
