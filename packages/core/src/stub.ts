/**
 * Stub discipline.
 *
 * DESIGN.md invariant 7: "The README documents only what runs. Nothing gets a
 * checkmark before it works end-to-end."
 *
 * The failure mode this file exists to prevent is a stub that returns its input
 * unchanged, or returns zeros. Both are indistinguishable from an operation that
 * ran and had no effect, which means the UI can show a control, the user can move
 * it, and nothing anywhere reports that the media was never touched. A stub must
 * therefore be *loud*: it throws.
 *
 * The one exception is a pipeline that composes many stages, where throwing would
 * take down thirteen working modules for one missing one. Those skip the stage and
 * report it — see `renderRawStack`'s `stubbedModules`. Skipping-and-reporting is
 * allowed; silently passing through is not.
 *
 * Every stub carries a `@stub` JSDoc tag. Audit with `pnpm stubs`.
 */

export type ImplementationStatus = 'implemented' | 'partial' | 'stub';

export class NotImplementedError extends Error {
  readonly feature: string;

  constructor(feature: string, reason?: string) {
    super(reason ? `${feature} is not implemented: ${reason}` : `${feature} is not implemented`);
    this.name = 'NotImplementedError';
    this.feature = feature;
  }
}

/**
 * Marks an unreachable-by-design code path in a stubbed function.
 *
 * Note for implementers: `noUnusedParameters` is enabled for this package, and a
 * body that throws before reading its arguments will fail typecheck. Discard them
 * with a `void x;` prelude rather than disabling the flag or renaming public
 * parameters that the UI reads for its own type inference.
 */
export function notImplemented(feature: string, reason?: string): never {
  throw new NotImplementedError(feature, reason);
}
