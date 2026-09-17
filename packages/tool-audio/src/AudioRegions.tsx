import type { BarrelRegionProps } from '@duckfoot/ui';
import { ActionButton, MonoValue } from '@duckfoot/ui';
import { formatTimecodeMs, orderedLanes } from '@duckfoot/core';
import { useAudio } from './AudioContext';
import { CorrelationMeter } from './components/CorrelationMeter';

export function AudioTransport({ doc, dispatch }: BarrelRegionProps) {
  const { view } = useAudio();

  return (
    <div className="df-transport-row">
      {/* Play / Stop controls */}
      <div style={{ display: 'flex', gap: '6px' }}>
        <button
          type="button"
          onClick={() => view.setIsPlaying(!view.isPlaying)}
          className="df-icon-button"
          style={{ width: '34px', height: '30px' }}
          title={view.isPlaying ? 'Pause' : 'Play'}
        >
          {view.isPlaying ? '❚❚' : '▶'}
        </button>
        <button
          type="button"
          onClick={() => {
            view.setIsPlaying(false);
            view.setPlayheadSeconds(doc.audio.selection?.inSeconds ?? 0);
          }}
          className="df-icon-button"
          style={{ width: '34px', height: '30px' }}
          title="Stop"
        >
          ■
        </button>
        <button
          type="button"
          onClick={() => view.setLoopSelection(!view.loopSelection)}
          className={`df-action${view.loopSelection ? ' df-chip--active' : ''}`}
          style={{
            height: '30px',
            padding: '0 11px',
            display: 'flex',
            alignItems: 'center',
            fontSize: '11px',
          }}
        >
          Loop selection
        </button>
        <button
          type="button"
          onClick={() => view.setPhaseSolo(!view.phaseSolo)}
          className={`df-action${view.phaseSolo ? ' df-chip--active' : ''}`}
          style={{
            height: '30px',
            padding: '0 11px',
            display: 'flex',
            alignItems: 'center',
            fontSize: '11px',
          }}
          title="Phase solo (monitor ref + active lane only)"
        >
          Phase solo
        </button>
      </div>

      <div style={{ width: '1px', height: '18px', background: 'var(--df-rule)', margin: '0 4px' }} />

      {/* Large playhead timecode */}
      <div style={{ display: 'flex', alignItems: 'baseline', gap: '6px' }}>
        <span
          className="df-mono"
          style={{ fontSize: '17px', fontWeight: 500, color: 'var(--df-text)' }}
        >
          {formatTimecodeMs(view.playheadSeconds)}
        </span>
        <span className="df-mono df-mono--dim" style={{ fontSize: '10px' }}>
          {`/ ${formatTimecodeMs(view.totalDurationSeconds)}`}
        </span>
      </div>

      {/* Region edit actions */}
      <div style={{ display: 'flex', gap: '6px', marginLeft: '12px' }}>
        <ActionButton
          disabled={!doc.audio.selection}
          onClick={() => dispatch({ type: 'audio/trimToSelection' })}
        >
          Trim to in/out
        </ActionButton>
        <ActionButton
          disabled={!doc.audio.selection}
          onClick={() => dispatch({ type: 'audio/deleteRegion' })}
        >
          Cut selection
        </ActionButton>
        <ActionButton
          disabled={!doc.audio.selection}
          onClick={() => dispatch({ type: 'audio/normalizeSelection', targetPeakDb: -0.3 })}
        >
          Normalize peak −0.3 dB
        </ActionButton>
      </div>

      <div className="df-transport-row__spacer" />

      {/* Peak meter */}
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: '9px',
          fontFamily: 'var(--df-font-mono)',
          fontSize: '10px',
          color: 'var(--df-text3)',
        }}
      >
        <span>peak</span>
        <div
          style={{
            position: 'relative',
            width: '120px',
            height: '7px',
            background: 'var(--df-inset)',
            borderRadius: '1px',
            overflow: 'hidden',
          }}
        >
          <div
            style={{
              position: 'absolute',
              left: 0,
              width: '74%',
              top: 0,
              bottom: 0,
              background: 'var(--df-in-point)',
            }}
          />
          <div
            style={{
              position: 'absolute',
              left: '74%',
              width: '14%',
              top: 0,
              bottom: 0,
              background: 'var(--df-brass)',
            }}
          />
        </div>
        <span style={{ color: 'var(--df-text-calm)' }}>−3.1 dB</span>
      </div>

      {/* Revert to original */}
      <ActionButton
        onClick={() => {
          // Revert non-destructive edits
          if (doc.audio.selection) {
            dispatch({ type: 'audio/clearSelection' });
          }
        }}
      >
        Revert to original
      </ActionButton>
    </div>
  );
}

export function AudioStatus({ doc }: BarrelRegionProps) {
  const lanes = orderedLanes(doc.audio);
  const voxLane = lanes.find((l) => l.name.toLowerCase().includes('vox'));
  const phaseLabel = voxLane?.phase.offsetSeconds
    ? `phase: ${voxLane.name} ${(voxLane.phase.offsetSeconds * 1000).toFixed(1)} ms`
    : 'phase: aligned';

  const bridge = typeof window !== 'undefined'
    ? (window as unknown as { __duckfoot?: { state?: { index: number } } }).__duckfoot
    : undefined;
  const undoCount = bridge?.state?.index ?? 0;

  return (
    <>
      <MonoValue tone="accent">{phaseLabel}</MonoValue>
      <MonoValue tone="default">{`${lanes.length} lanes · 48 kHz · 24-bit`}</MonoValue>
      <span className="df-mono df-mono--dim">|</span>
      <MonoValue tone="dim">{`undo ${undoCount}`}</MonoValue>
    </>
  );
}

export function AudioShelfPanels({ doc }: BarrelRegionProps) {
  const lanes = orderedLanes(doc.audio);
  const refLane = lanes.find((l) => l.isPhaseReference) ?? lanes[0] ?? null;
  const otherLanes = lanes.filter((l) => l.id !== refLane?.id);

  return (
    <div className="df-shelf-panels">
      <CorrelationMeter referenceLane={refLane} otherLanes={otherLanes} />
    </div>
  );
}
