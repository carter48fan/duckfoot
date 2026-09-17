import { createContext, useContext, useMemo, useState } from 'react';
import type { ReactNode } from 'react';
import type { AssetItem, RawStack } from '@duckfoot/core';
import type { BarrelRegionProps } from '@duckfoot/ui';
import { usePhotoRender } from './usePhotoRender';
import type { PhotoRender } from './usePhotoRender';

export interface PhotoViewState {
  /** View state, not document state — none of this goes on the undo stack. */
  expandedModule: string | null;
  setExpandedModule: (id: string | null) => void;
  inspectorTab: 'pipeline' | 'masks' | 'presets';
  setInspectorTab: (tab: 'pipeline' | 'masks' | 'presets') => void;
  scopeTab: 'hist' | 'wave' | 'vect';
  setScopeTab: (tab: 'hist' | 'wave' | 'vect') => void;
  mode: 'lighttable' | 'develop';
  setMode: (mode: 'lighttable' | 'develop') => void;
  /** 0..1 position of the before/after divider. */
  split: number;
  setSplit: (value: number) => void;
  /** Uncommitted stack during a drag — see the preview/commit split in useDragGesture. */
  previewStack: RawStack | null;
  setPreviewStack: (stack: RawStack | null) => void;
  snapshots: { name: string; stack: RawStack }[];
  setSnapshots: (snapshots: { name: string; stack: RawStack }[]) => void;
}

const Context = createContext<{ render: PhotoRender; view: PhotoViewState; asset: AssetItem | null } | null>(
  null
);

export function usePhoto() {
  const value = useContext(Context);
  if (!value) throw new Error('Photo regions must be rendered inside PhotoProvider');
  return value;
}

export function PhotoProvider({ doc, assets, activeAssetId, children }: BarrelRegionProps & { children: ReactNode }) {
  const [expandedModule, setExpandedModule] = useState<string | null>('toneCurve');
  const [inspectorTab, setInspectorTab] = useState<'pipeline' | 'masks' | 'presets'>('pipeline');
  const [scopeTab, setScopeTab] = useState<'hist' | 'wave' | 'vect'>('hist');
  const [mode, setMode] = useState<'lighttable' | 'develop'>('develop');
  const [split, setSplit] = useState(0.5);
  const [previewStack, setPreviewStack] = useState<RawStack | null>(null);
  const [snapshots, setSnapshots] = useState<{ name: string; stack: RawStack }[]>([]);

  const asset = useMemo(() => {
    const direct = assets.find((item) => item.id === activeAssetId && item.type === 'photo');
    if (direct) return direct;
    const fromDoc = assets.find((item) => item.id === doc.photo.assetId && item.type === 'photo');
    if (fromDoc) return fromDoc;
    return assets.find((item) => item.type === 'photo') ?? null;
  }, [assets, activeAssetId, doc.photo.assetId]);

  // A drag renders through previewStack; everything else through the document. This
  // is what lets a slider show its result live while producing one history entry.
  const stack = previewStack ?? doc.photo.stack;
  const render = usePhotoRender(asset, stack);

  const view: PhotoViewState = {
    expandedModule,
    setExpandedModule,
    inspectorTab,
    setInspectorTab,
    scopeTab,
    setScopeTab,
    mode,
    setMode,
    split,
    setSplit,
    previewStack,
    setPreviewStack,
    snapshots,
    setSnapshots,
  };

  return <Context.Provider value={{ render, view, asset }}>{children}</Context.Provider>;
}
