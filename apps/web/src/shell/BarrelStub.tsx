import type { BarrelRegionProps } from '@duckfoot/ui';

/**
 * A barrel that exists in the frame but has not been rebuilt.
 *
 * The Video barrel's component is 544 lines of Tailwind class strings with no
 * Tailwind installed; mounting it would put unstyled stacked divs next to two
 * pixel-exact barrels. It stays in the workspace as reference and in `pnpm
 * typecheck`, but it is not imported here.
 *
 * Keeping the slot occupied is not cosmetic: switching Photo → Video → Photo and
 * finding the history intact is how invariant 1 gets demonstrated, and that needs a
 * third barrel to switch to.
 */
export function makeBarrelStub(title: string, note: string) {
  function Workspace(_props: BarrelRegionProps) {
    return (
      <div className="df-barrel-stub">
        <span className="df-barrel-stub__title">{title}</span>
        <span className="df-barrel-stub__note df-mono">{note}</span>
      </div>
    );
  }
  function Empty(_props: BarrelRegionProps) {
    return null;
  }
  return { Workspace, Inspector: Empty, Transport: Empty, ShelfPanels: Empty, Status: Empty };
}
