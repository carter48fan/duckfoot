import type { BarrelId } from '@duckfoot/core';
import type { BarrelDefinition } from '@duckfoot/ui';

/**
 * Switching barrels is a view change and nothing else — no unmount, no document
 * touched, so nothing on the undo stack either. That is the whole point of the
 * region registry, and it is what makes DESIGN.md's acceptance test pass: edit
 * Photo, switch to Audio, come back, and the history is untouched.
 */
export function BarrelSwitcher({
  barrels,
  active,
  onSelect,
}: {
  barrels: readonly BarrelDefinition[];
  active: BarrelId;
  onSelect: (id: BarrelId) => void;
}) {
  return (
    <div className="df-switcher" role="tablist" aria-label="Barrel">
      {barrels.map((barrel) => (
        <button
          key={barrel.id}
          type="button"
          role="tab"
          aria-selected={barrel.id === active}
          className={`df-switcher__tab${barrel.id === active ? ' df-switcher__tab--active' : ''}`}
          onClick={() => onSelect(barrel.id)}
        >
          {barrel.label}
        </button>
      ))}
    </div>
  );
}
