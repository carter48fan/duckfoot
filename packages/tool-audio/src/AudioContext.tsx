import { createContext, useContext, useEffect, useMemo, useState } from 'react';
import type { ReactNode } from 'react';
import type { EnvelopeNode } from '@duckfoot/core';
import type { BarrelRegionProps } from '@duckfoot/ui';

export interface AudioViewState {
  playheadSeconds: number;
  setPlayheadSeconds: (t: number | ((prev: number) => number)) => void;
  isPlaying: boolean;
  setIsPlaying: (p: boolean | ((prev: boolean) => boolean)) => void;
  loopSelection: boolean;
  setLoopSelection: (loop: boolean | ((prev: boolean) => boolean)) => void;
  phaseSolo: boolean;
  setPhaseSolo: (solo: boolean | ((prev: boolean) => boolean)) => void;
  activeLaneId: string | null;
  setActiveLaneId: (id: string | null) => void;
  previewOffsetSeconds: Record<string, number>;
  setPreviewOffsetSeconds: (map: Record<string, number>) => void;
  previewRotationDegrees: Record<string, number>;
  setPreviewRotationDegrees: (map: Record<string, number>) => void;
  previewGainDb: Record<string, number>;
  setPreviewGainDb: (map: Record<string, number>) => void;
  previewNodes: EnvelopeNode[] | null;
  setPreviewNodes: (nodes: EnvelopeNode[] | null) => void;
  totalDurationSeconds: number;
}

interface AudioContextValue {
  view: AudioViewState;
}

const Context = createContext<AudioContextValue | null>(null);

export function useAudio() {
  const value = useContext(Context);
  if (!value) throw new Error('Audio components must be rendered inside AudioProvider');
  return value;
}

export function AudioProvider({ doc, children }: BarrelRegionProps & { children: ReactNode }) {
  // Turn 2 fixture has 3:24.000 duration (204.0s) and playhead at 0:48.940
  const totalDurationSeconds = doc.audio.durationSeconds > 0 ? doc.audio.durationSeconds : 204.0;
  const [playheadSeconds, setPlayheadSeconds] = useState(48.94);
  const [isPlaying, setIsPlaying] = useState(false);
  const [loopSelection, setLoopSelection] = useState(true);
  const [phaseSolo, setPhaseSolo] = useState(false);

  // Focus active lane (default to vox or first lane)
  const defaultActiveId = doc.audio.laneOrder.includes('vox')
    ? 'vox'
    : doc.audio.laneOrder[0] ?? null;
  const [activeLaneId, setActiveLaneId] = useState<string | null>(defaultActiveId);

  // Uncommitted preview state during drags
  const [previewOffsetSeconds, setPreviewOffsetSeconds] = useState<Record<string, number>>({});
  const [previewRotationDegrees, setPreviewRotationDegrees] = useState<Record<string, number>>({});
  const [previewGainDb, setPreviewGainDb] = useState<Record<string, number>>({});
  const [previewNodes, setPreviewNodes] = useState<EnvelopeNode[] | null>(null);

  useEffect(() => {
    const handler = () => setIsPlaying((prev) => !prev);
    window.addEventListener('df:toggle-play', handler);
    return () => window.removeEventListener('df:toggle-play', handler);
  }, []);

  // Live playback transport ticker
  useEffect(() => {
    if (!isPlaying) return;
    let lastTime = performance.now();
    let animId: number;

    const tick = (now: number) => {
      const delta = (now - lastTime) / 1000;
      lastTime = now;

      setPlayheadSeconds((prev) => {
        let next = prev + delta;
        const sel = doc.audio.selection;
        if (loopSelection && sel && next >= sel.outSeconds) {
          next = sel.inSeconds;
        } else if (next >= totalDurationSeconds) {
          next = 0;
        }
        return next;
      });

      animId = requestAnimationFrame(tick);
    };

    animId = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(animId);
  }, [isPlaying, loopSelection, doc.audio.selection, totalDurationSeconds]);

  const view: AudioViewState = useMemo(
    () => ({
      playheadSeconds,
      setPlayheadSeconds,
      isPlaying,
      setIsPlaying,
      loopSelection,
      setLoopSelection,
      phaseSolo,
      setPhaseSolo,
      activeLaneId,
      setActiveLaneId,
      previewOffsetSeconds,
      setPreviewOffsetSeconds,
      previewRotationDegrees,
      setPreviewRotationDegrees,
      previewGainDb,
      setPreviewGainDb,
      previewNodes,
      setPreviewNodes,
      totalDurationSeconds,
    }),
    [
      playheadSeconds,
      isPlaying,
      loopSelection,
      phaseSolo,
      activeLaneId,
      previewOffsetSeconds,
      previewRotationDegrees,
      previewGainDb,
      previewNodes,
      totalDurationSeconds,
    ]
  );

  return <Context.Provider value={{ view }}>{children}</Context.Provider>;
}
