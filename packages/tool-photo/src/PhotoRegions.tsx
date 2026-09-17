import type { BarrelRegionProps } from '@duckfoot/ui';
import { ActionButton, MonoValue, PanelHeading } from '@duckfoot/ui';
import { enabledModuleCount, visibleHistory } from '@duckfoot/core';
import { usePhoto } from './PhotoContext';

export function PhotoTransport({ doc, dispatch, selectedAssetIds }: BarrelRegionProps) {
  const { view } = usePhoto();
  const targets = selectedAssetIds.length > 0 ? selectedAssetIds : [];

  return (
    <div className="df-transport-row">
      <ActionButton onClick={() => dispatch({ type: 'photo/resetAllModules' })}>
        Reset all modules
      </ActionButton>
      <ActionButton
        onClick={() =>
          view.setSnapshots([
            ...view.snapshots,
            { name: `snapshot ${view.snapshots.length + 1}`, stack: structuredClone(doc.photo.stack) },
          ])
        }
      >
        Copy stack
      </ActionButton>
      <ActionButton
        disabled={targets.length === 0}
        onClick={() =>
          dispatch({
            type: 'photo/pasteStack',
            stack: structuredClone(doc.photo.stack),
            targetAssetIds: targets,
          })
        }
      >
        {targets.length > 0 ? `Paste to ${targets.length} selected` : 'Paste to selection'}
      </ActionButton>
      <div className="df-transport-row__spacer" />
      <MonoValue tone="dim">edits are non-destructive · the source is never written</MonoValue>
    </div>
  );
}

export function PhotoStatus({ doc }: BarrelRegionProps) {
  const { render, asset } = usePhoto();
  return (
    <>
      <MonoValue tone="accent">{asset ? 'raster · 8-bit sRGB' : 'no image'}</MonoValue>
      <MonoValue tone="dim">
        {render.sourceWidth ? `${render.sourceWidth} × ${render.sourceHeight}` : '—'}
      </MonoValue>
      <MonoValue tone="dim">{`${enabledModuleCount(doc.photo.stack)} modules on`}</MonoValue>
    </>
  );
}

export function PhotoShelfPanels({ doc, dispatch, appState }: BarrelRegionProps & { appState?: unknown }) {
  const { view } = usePhoto();
  void appState;

  return (
    <div className="df-shelf-panels">
      <PanelHeading trailing={<span>{view.snapshots.length}</span>}>SNAPSHOTS</PanelHeading>
      <div className="df-snapshots">
        <div className="df-snapshot df-snapshot--live">
          <span>current</span>
          <MonoValue tone="accent">live</MonoValue>
        </div>
        {view.snapshots.map((snapshot, index) => (
          <button
            key={index}
            type="button"
            className="df-snapshot"
            onClick={() =>
              dispatch({ type: 'photo/restoreSnapshot', name: snapshot.name, stack: snapshot.stack })
            }
          >
            <span>{snapshot.name}</span>
            <MonoValue tone="dim">{String.fromCharCode(65 + index)}</MonoValue>
          </button>
        ))}
      </div>
      <HistoryPanel doc={doc} />
    </div>
  );
}

/**
 * The HISTORY panel reads the undo stack directly, which is only possible because
 * every mutation is a command — nothing edits the document behind the stack's back.
 */
function HistoryPanel({ doc }: { doc: unknown }) {
  void doc;
  const entries = typeof window !== 'undefined' ? readHistory() : [];
  return (
    <>
      <PanelHeading trailing={<span>{entries.length}</span>}>HISTORY</PanelHeading>
      <div className="df-history">
        {entries.slice(0, 8).map((entry, index) => (
          <div
            key={index}
            className={`df-history__row${index === 0 ? ' df-history__row--now' : ''}`}
            style={{ cursor: index === 0 ? 'default' : 'pointer' }}
            onClick={() => {
              if (index > 0) {
                const bridge = (
                  window as unknown as {
                    __duckfoot?: { state?: { index: number }; jumpTo?: (idx: number) => void };
                  }
                ).__duckfoot;
                if (bridge?.state && bridge.jumpTo) {
                  bridge.jumpTo(bridge.state.index - index);
                }
              }
            }}
            title={index === 0 ? 'Current state' : 'Jump to this snapshot in history'}
          >
            <span>{entry.label}</span>
            <MonoValue tone={index === 0 ? 'accent' : 'dim'}>
              {index === 0 ? 'now' : String(entries.length - index)}
            </MonoValue>
          </div>
        ))}
      </div>
    </>
  );
}

function readHistory(): { label: string }[] {
  const bridge = (window as unknown as { __duckfoot?: { state?: Parameters<typeof visibleHistory>[0] } })
    .__duckfoot;
  if (!bridge?.state) return [];
  return visibleHistory(bridge.state);
}
