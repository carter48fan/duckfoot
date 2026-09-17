import type { ComponentType, ReactNode } from 'react';
import type { AssetItem, BarrelId, Command, DuckfootDocument } from '@duckfoot/core';

/**
 * What every barrel region receives.
 *
 * DESIGN.md invariant 1: "A tool never owns its document. State lives above the
 * components; all three stay mounted." So a region gets the document and a way to
 * dispatch commands against it, and holds no document state of its own. What it may
 * hold in useState is *view* state — which tab is open, whether a module row is
 * expanded — because none of that changes the media.
 *
 * Selection is passed separately from the document for the same reason: selecting a
 * shelf row does not mutate anything, so it is not a command and does not belong on
 * the undo stack.
 */
export interface BarrelRegionProps {
  doc: DuckfootDocument;
  dispatch: (command: Command) => void;
  assets: AssetItem[];
  selectedAssetIds: string[];
  activeAssetId: string | null;
  onSelectAsset: (assetId: string, additive: boolean) => void;
}

/**
 * A barrel, as five slots in the shared frame.
 *
 * The shell renders every barrel's region into each slot and hides the inactive
 * ones. Nothing unmounts on a barrel switch, which is what makes invariant 1
 * structural rather than something a future contributor has to remember. The
 * previous build conditionally rendered whole tools, and DESIGN.md records what that
 * cost: "every barrel switch silently destroyed the user's work."
 */
export interface BarrelDefinition {
  id: BarrelId;
  label: string;
  /**
   * Optional context provider, mounted by the shell around the entire frame.
   *
   * A barrel's regions live in four different parts of the layout but share derived
   * state — the Photo workspace draws the developed preview, the Inspector scopes
   * it, the status strip reports its clipping. Decoding and rendering once and
   * sharing the result is why this exists.
   *
   * It wraps the whole frame rather than each region, so it stays mounted across
   * barrel switches like everything else.
   */
  Provider?: ComponentType<BarrelRegionProps & { children: ReactNode }>;
  Workspace: ComponentType<BarrelRegionProps>;
  Inspector: ComponentType<BarrelRegionProps>;
  Transport: ComponentType<BarrelRegionProps>;
  ShelfPanels: ComponentType<BarrelRegionProps>;
  Status: ComponentType<BarrelRegionProps>;
}
