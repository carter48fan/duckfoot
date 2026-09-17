import { useRef } from 'react';
import type { Selection } from '@duckfoot/core';
import { useDragGesture } from '@duckfoot/ui';
import { SelectionOverlay } from './SelectionOverlay';

export interface TimeRulerProps {
  totalDurationSeconds: number;
  selection: Selection | null;
  playheadSeconds: number;
  onSeek: (timeSeconds: number) => void;
}

export function TimeRuler({
  totalDurationSeconds,
  selection,
  playheadSeconds,
  onSeek,
}: TimeRulerProps) {
  const surfaceRef = useRef<HTMLDivElement>(null);

  const seekFromPointer = (clientX: number) => {
    const surface = surfaceRef.current;
    if (!surface || totalDurationSeconds <= 0) return;
    const rect = surface.getBoundingClientRect();
    const ratio = Math.max(0, Math.min(1, (clientX - rect.left) / rect.width));
    onSeek(ratio * totalDurationSeconds);
  };

  const startScrub = useDragGesture({
    onStart: (e) => {
      seekFromPointer(e.clientX);
      return {};
    },
    onMove: (_state, e) => {
      seekFromPointer(e.clientX);
    },
    onCommit: (_state, e) => {
      seekFromPointer(e.clientX);
    },
  });

  // Calculate ticks (every 15 or 30 seconds)
  const tickInterval = totalDurationSeconds > 180 ? 30 : 15;
  const tickCount = Math.floor(totalDurationSeconds / tickInterval);
  const ticks = Array.from({ length: tickCount + 1 }, (_, i) => i * tickInterval);

  const playheadPercent =
    totalDurationSeconds > 0
      ? Math.max(0, Math.min(100, (playheadSeconds / totalDurationSeconds) * 100))
      : 0;

  return (
    <div
      className="df-audio-ruler"
      style={{
        display: 'flex',
        alignItems: 'stretch',
        gap: '12px',
        userSelect: 'none',
      }}
    >
      {/* Left spacer matching lane label column */}
      <div
        style={{
          width: 'var(--df-lane-label-w, 110px)',
          flexShrink: 0,
          display: 'flex',
          alignItems: 'flex-end',
          paddingBottom: '2px',
        }}
      >
        <span className="df-mono df-mono--dim" style={{ fontSize: '9px', letterSpacing: '0.08em' }}>
          TIME
        </span>
      </div>

      {/* Center ruler surface */}
      <div
        ref={surfaceRef}
        onPointerDown={startScrub}
        style={{
          flex: 1,
          minWidth: 0,
          position: 'relative',
          height: '20px',
          borderBottom: '1px solid var(--df-rule)',
          cursor: 'pointer',
        }}
      >
        {/* Ticks and time marks */}
        {ticks.map((t) => {
          const pct = (t / totalDurationSeconds) * 100;
          const mins = Math.floor(t / 60);
          const secs = (t % 60).toString().padStart(2, '0');
          return (
            <div
              key={t}
              style={{
                position: 'absolute',
                left: `${pct}%`,
                top: 0,
                bottom: 0,
                display: 'flex',
                flexDirection: 'column',
                pointerEvents: 'none',
              }}
            >
              <div style={{ height: '4px', width: '1px', background: 'var(--df-text-ghost)' }} />
              <span
                className="df-mono"
                style={{
                  fontSize: '9px',
                  color: 'var(--df-text5)',
                  transform: 'translateX(-50%)',
                  paddingTop: '2px',
                }}
              >
                {`${mins}:${secs}`}
              </span>
            </div>
          );
        })}

        {/* Selection overlay */}
        <SelectionOverlay
          selection={selection}
          totalDurationSeconds={totalDurationSeconds}
          showLabels={true}
        />

        {/* Playhead marker */}
        <div
          style={{
            position: 'absolute',
            left: `${playheadPercent}%`,
            top: 0,
            bottom: '-12px',
            width: '1px',
            background: 'var(--df-playhead)',
            zIndex: 10,
            pointerEvents: 'none',
          }}
        >
          {/* Top triangle */}
          <div
            style={{
              position: 'absolute',
              top: 0,
              left: '-4px',
              width: 0,
              height: 0,
              borderLeft: '4px solid transparent',
              borderRight: '4px solid transparent',
              borderTop: '6px solid var(--df-playhead)',
            }}
          />
        </div>
      </div>

      {/* Right spacer matching lane control column */}
      <div
        style={{
          width: 'var(--df-lane-ctl-w, 196px)',
          flexShrink: 0,
        }}
      />
    </div>
  );
}
