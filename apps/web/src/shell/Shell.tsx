import type { ReactNode } from 'react';
import type { BarrelId } from '@duckfoot/core';
import type { BarrelDefinition, BarrelRegionProps } from '@duckfoot/ui';
import { BrandLockup } from './BrandLockup';
import { BarrelSwitcher } from './BarrelSwitcher';

/**
 * The shared frame. DESIGN.md: "One frame, three fills… A user who learns one
 * barrel has already learned the furniture of the other two."
 *
 * Every barrel's regions are rendered into every slot; the inactive ones carry
 * `hidden`. That is what keeps all three mounted — the failure invariant 1 exists to
 * prevent is a barrel switch destroying work, and it cannot happen if nothing
 * unmounts.
 */
export function Shell({
  barrels,
  active,
  onSelectBarrel,
  regionProps,
  shelf,
  shelfOpen,
  onToggleShelf,
  inspectorOpen,
  onToggleInspector,
  storageLabel,
}: {
  barrels: readonly BarrelDefinition[];
  active: BarrelId;
  onSelectBarrel: (id: BarrelId) => void;
  regionProps: BarrelRegionProps;
  shelf: (panels: ReactNode) => ReactNode;
  shelfOpen: boolean;
  onToggleShelf: () => void;
  inspectorOpen: boolean;
  onToggleInspector: () => void;
  storageLabel: string;
}) {
  const panels = barrels.map((barrel) => (
    <div key={barrel.id} hidden={barrel.id !== active}>
      <barrel.ShelfPanels {...regionProps} />
    </div>
  ));

  // Providers nest around the whole frame so a barrel's shared state survives a
  // switch exactly as its regions do.
  const withProviders = (children: ReactNode): ReactNode =>
    barrels.reduceRight<ReactNode>((inner, barrel) => {
      if (!barrel.Provider) return inner;
      return <barrel.Provider {...regionProps}>{inner}</barrel.Provider>;
    }, children);

  return withProviders(
    <div className="df-frame">
      {/* The knurled brass rule — machined edge, turn-2 chrome. */}
      <div className="df-knurl" aria-hidden="true" />

      <header className="df-header">
        <BrandLockup />
        <BarrelSwitcher barrels={barrels} active={active} onSelect={onSelectBarrel} />
        <div className="df-header__spacer" />
        <div className="df-header__status">
          {barrels.map((barrel) => (
            <span key={barrel.id} hidden={barrel.id !== active}>
              <barrel.Status {...regionProps} />
            </span>
          ))}
          <span className="df-mono df-mono--dim">{storageLabel}</span>
        </div>
      </header>

      <div className="df-body">
        {shelfOpen ? (
          shelf(panels)
        ) : (
          <button type="button" className="df-rail" onClick={onToggleShelf} title="Show Shelf">
            <span className="df-rail__label">SHELF</span>
          </button>
        )}

        <main className="df-workspace">
          {barrels.map((barrel) => (
            <div key={barrel.id} className="df-workspace__fill" hidden={barrel.id !== active}>
              <barrel.Workspace {...regionProps} />
            </div>
          ))}
        </main>

        {inspectorOpen ? (
          <aside className="df-inspector">
            {barrels.map((barrel) => (
              <div key={barrel.id} className="df-inspector__fill" hidden={barrel.id !== active}>
                <barrel.Inspector {...regionProps} />
              </div>
            ))}
          </aside>
        ) : (
          <button
            type="button"
            className="df-rail df-rail--right"
            onClick={onToggleInspector}
            title="Show Inspector"
          >
            <span className="df-rail__label">INSPECTOR</span>
          </button>
        )}
      </div>

      <footer className="df-transport">
        {barrels.map((barrel) => (
          <div key={barrel.id} className="df-transport__fill" hidden={barrel.id !== active}>
            <barrel.Transport {...regionProps} />
          </div>
        ))}
      </footer>
    </div>
  );
}
