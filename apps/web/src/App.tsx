import { useCallback, useEffect, useMemo, useState, useSyncExternalStore } from 'react';
import type { BarrelId, Command } from '@duckfoot/core';
import {
  OpfsStorageDriver,
  MemoryStorageDriver,
  createAppState,
  currentDocument,
  execute,
  undo as applyUndo,
  redo as applyRedo,
  jumpTo as applyJumpTo,
} from '@duckfoot/core';
import type { BarrelRegionProps } from '@duckfoot/ui';
import { Shell } from './shell/Shell';
import { Shelf } from './shell/Shelf';
import { BARRELS } from './shell/barrels';
import { usePanelState } from './shell/usePanelState';
import { useHotkeys } from './shell/useHotkeys';
import { useAssetIngest } from './shell/useAssetIngest';
import { createFixtureDocument } from './dev/fixtures';

// In-memory document store with snapshot undo history
let globalState = createAppState(createFixtureDocument());
const listeners = new Set<() => void>();

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function getSnapshot() {
  return globalState;
}

function dispatchCommand(command: Command) {
  globalState = execute(globalState, command);
  listeners.forEach((l) => l());
}

function undoAction() {
  globalState = applyUndo(globalState);
  listeners.forEach((l) => l());
}

function redoAction() {
  globalState = applyRedo(globalState);
  listeners.forEach((l) => l());
}

function jumpToAction(index: number) {
  globalState = applyJumpTo(globalState, index);
  listeners.forEach((l) => l());
}

export function App() {
  const state = useSyncExternalStore(subscribe, getSnapshot);
  const doc = currentDocument(state);

  const [activeBarrel, setActiveBarrel] = useState<BarrelId>('photo');
  const [shelfOpen, toggleShelf] = usePanelState('df:panel:shelf', true);
  const [inspectorOpen, toggleInspector] = usePanelState('df:panel:inspector', true);

  // Asset selection
  const [selectedAssetIds, setSelectedAssetIds] = useState<string[]>(['asset-4412']);
  const [activeAssetId, setActiveAssetId] = useState<string | null>('asset-4412');

  const storage = useMemo(() => {
    return 'storage' in navigator && 'getDirectory' in navigator.storage
      ? new OpfsStorageDriver()
      : new MemoryStorageDriver();
  }, []);

  const [storageLabel, setStorageLabel] = useState<string>('OPFS 412 MB');

  useEffect(() => {
    storage.usage().then((usage) => {
      if (usage) {
        const mb = Math.round(usage.usedBytes / (1024 * 1024));
        setStorageLabel(`${storage.name.toUpperCase()} ${mb} MB`);
      } else {
        setStorageLabel(`${storage.name.toUpperCase()} —`);
      }
    });
  }, [storage]);

  const handleSelectAsset = useCallback((id: string, additive: boolean) => {
    setActiveAssetId(id);
    setSelectedAssetIds((prev) => {
      if (!additive) return [id];
      return prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id];
    });
    const found = doc.suite.assets.find((a) => a.id === id);
    if (found && found.type === 'photo') {
      dispatchCommand({ type: 'photo/openAsset', assetId: id, kind: 'raw' });
    }
  }, [doc.suite.assets]);

  const ingest = useAssetIngest(dispatchCommand);

  // Global hotkeys
  useHotkeys({
    onUndo: undoAction,
    onRedo: redoAction,
    onTogglePlay: () => {
      window.dispatchEvent(new CustomEvent('df:toggle-play'));
    },
    onSetIn: () => {
      if (activeBarrel === 'audio') {
        const t = 42.118; // or current playhead
        const outSec = doc.audio.selection?.outSeconds ?? 78.226;
        dispatchCommand({ type: 'audio/setSelection', inSeconds: t, outSeconds: outSec });
      }
    },
    onSetOut: () => {
      if (activeBarrel === 'audio') {
        const inSec = doc.audio.selection?.inSeconds ?? 42.118;
        const t = 78.226; // or current playhead
        dispatchCommand({ type: 'audio/setSelection', inSeconds: inSec, outSeconds: t });
      }
    },
    onAutoAlign: () => {
      if (activeBarrel === 'audio') {
        const vox = doc.audio.lanes['lane-vox'];
        if (vox) {
          dispatchCommand({
            type: 'audio/setLanePhase',
            laneId: vox.id,
            patch: { offsetSeconds: -0.00142, rotationDegrees: 38 },
          });
        }
      }
    },
    onDeleteRegion: () => {
      if (activeBarrel === 'audio' && doc.audio.selection) {
        dispatchCommand({ type: 'audio/deleteRegion' });
      }
    },
    onEscape: () => {
      // Esc cancels active gestures
    },
  });

  // Expose dev bridge for testing invariant 1 & undo/redo
  useEffect(() => {
    (window as unknown as { __duckfoot?: unknown }).__duckfoot = {
      state,
      doc,
      dispatch: dispatchCommand,
      undo: undoAction,
      redo: redoAction,
      jumpTo: jumpToAction,
      barrels: BARRELS,
    };
  }, [state, doc]);

  const regionProps: BarrelRegionProps = useMemo(
    () => ({
      doc,
      dispatch: dispatchCommand,
      assets: doc.suite.assets,
      selectedAssetIds,
      activeAssetId,
      onSelectAsset: handleSelectAsset,
    }),
    [doc, selectedAssetIds, activeAssetId, handleSelectAsset]
  );

  return (
    <Shell
      barrels={BARRELS}
      active={activeBarrel}
      onSelectBarrel={setActiveBarrel}
      regionProps={regionProps}
      shelfOpen={shelfOpen}
      onToggleShelf={toggleShelf}
      inspectorOpen={inspectorOpen}
      onToggleInspector={toggleInspector}
      storageLabel={storageLabel}
      shelf={(panels) => (
        <Shelf
          assets={doc.suite.assets}
          selectedIds={selectedAssetIds}
          activeId={activeAssetId}
          onSelect={handleSelectAsset}
          onImport={ingest}
          panels={panels}
          label={`${doc.suite.assets.length} items`}
        />
      )}
    />
  );
}
