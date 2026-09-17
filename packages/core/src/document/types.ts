import type { AssetItem } from '../types';
import type { PhotoDocument } from './photo';
import type { AudioDocument } from './audio';

/**
 * The Shelf — DESIGN.md calls it "the stock". One asset list, shared by all three
 * barrels, and the only route between them.
 *
 * Only mutations live here. Which rows are *selected* is view state and belongs in
 * the shell, because selecting a row does not change the media.
 */
export interface SuiteDocument {
  assets: AssetItem[];
}

export function createSuiteDocument(): SuiteDocument {
  return { assets: [] };
}

/**
 * One document above all three barrels.
 *
 * DESIGN.md invariant 1: "A tool never owns its document. State lives above the
 * components; all three stay mounted." A single document with one undo stack is
 * what makes a barrel switch a pure view change — there is nothing per-barrel to
 * destroy, so the failure that invariant was written about cannot recur.
 */
export interface DuckfootDocument {
  suite: SuiteDocument;
  photo: PhotoDocument;
  audio: AudioDocument;
}
