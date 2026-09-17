import { useRef } from 'react';
import type { Lane, Selection } from '@duckfoot/core';
import { IconButton, Slider } from '@duckfoot/ui';
import { SelectionOverlay } from './SelectionOverlay';
import { Waveform } from './Waveform';

export interface LaneRowProps {
  lane: Lane;
  active: boolean;
  onSelect: () => void;
  totalDurationSeconds: number;
  selection: Selection | null;
  playheadSeconds: number;
  onGainCommit: (gainDb: number) => void;
  onGainPreview: (gainDb: number) => void;
  previewGainDb?: number;
  onToggleMute: () => void;
  onToggleSolo: () => void;
  onTogglePolarity: () => void;
  onSeek: (timeSeconds: number) => void;
}

export function LaneRow({
  lane,
  active,
  onSelect,
  totalDurationSeconds,
  selection,
  playheadSeconds,
  onGainCommit,
  onGainPreview,
  previewGainDb,
  onToggleMute,
  onToggleSolo,
  onTogglePolarity,
  onSeek,
}: LaneRowProps) {
  const wellRef = useRef<HTMLDivElement>(null);
  const effectiveGain = previewGainDb ?? lane.gainDb;

  const playheadPercent =
    totalDurationSeconds > 0
      ? Math.max(0, Math.min(100, (playheadSeconds / totalDurationSeconds) * 100))
      : 0;

  // Build phase badge text if lane has active phase adjustments
  const phaseParts: string[] = [];
  if (lane.phase.polarityInverted) phaseParts.push('ø');
  if (lane.phase.offsetSeconds !== 0) {
    const ms = lane.phase.offsetSeconds * 1000;
    phaseParts.push(`${ms > 0 ? '+' : ''}${ms.toFixed(2)} ms`);
  }
  if (lane.phase.rotationDegrees !== 0) {
    const deg = lane.phase.rotationDegrees;
    phaseParts.push(`${deg > 0 ? '+' : ''}${deg}°`);
  }
  const phaseBadgeText = phaseParts.join(' · ');

  const handlePointerDown = (e: React.PointerEvent) => {
    onSelect();
    const well = wellRef.current;
    if (!well || totalDurationSeconds <= 0) return;
    const rect = well.getBoundingClientRect();
    const ratio = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
    onSeek(ratio * totalDurationSeconds);
  };

  const gainDisplay = `${effectiveGain > 0 ? '+' : ''}${effectiveGain.toFixed(1)} dB`;

  return (
    <div
      className={`df-lane-row${active ? ' df-lane-row--active' : ''}`}
      style={{
        display: 'flex',
        alignItems: 'stretch',
        height: '66px',
        gap: '12px',
        userSelect: 'none',
      }}
    >
      {/* Column 1: Lane label */}
      <div
        onClick={onSelect}
        style={{
          width: 'var(--df-lane-label-w, 110px)',
          flexShrink: 0,
          padding: '6px 8px',
          display: 'flex',
          flexDirection: 'column',
          justifyContent: 'center',
          gap: '3px',
          cursor: 'pointer',
          borderRadius: '2px',
          background: active ? 'var(--df-surface-raised, #181818)' : 'transparent',
          border: active ? '1px solid var(--df-rule, #262626)' : '1px solid transparent',
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
          <span
            style={{
              fontSize: '12px',
              fontWeight: 600,
              color: active ? 'var(--df-text)' : 'var(--df-text-calm)',
            }}
          >
            {lane.name}
          </span>
          {lane.isPhaseReference ? (
            <span
              className="df-mono"
              style={{
                fontSize: '8px',
                color: 'var(--df-brass)',
                background: 'var(--df-brass-surface)',
                border: '1px solid var(--df-brass-edge)',
                padding: '1px 3px',
                borderRadius: '2px',
                letterSpacing: '0.05em',
              }}
            >
              REF
            </span>
          ) : null}
        </div>
        <span className="df-mono df-mono--dim" style={{ fontSize: '9px' }}>
          48 kHz · 24-bit
        </span>
      </div>

      {/* Column 2: Waveform well */}
      <div
        ref={wellRef}
        onPointerDown={handlePointerDown}
        style={{
          flex: 1,
          minWidth: 0,
          position: 'relative',
          background: 'var(--df-well, #101010)',
          border: '1px solid var(--df-rule-soft, #1f1f1f)',
          borderRadius: '2px',
          cursor: 'pointer',
          overflow: 'hidden',
          display: 'flex',
          alignItems: 'center',
        }}
      >
        <Waveform
          seed={lane.name}
          muted={lane.muted}
          active={!lane.muted && (lane.soloed || active)}
          height={56}
        />

        {/* Shared selection overlay */}
        <SelectionOverlay
          selection={selection}
          totalDurationSeconds={totalDurationSeconds}
          showLabels={false}
        />

        {/* Playhead line */}
        <div
          style={{
            position: 'absolute',
            left: `${playheadPercent}%`,
            top: 0,
            bottom: 0,
            width: '1px',
            background: 'var(--df-playhead)',
            zIndex: 10,
            pointerEvents: 'none',
          }}
        />

        {/* Phase offset badge overlay */}
        {phaseBadgeText ? (
          <div
            className="df-mono"
            style={{
              position: 'absolute',
              top: 6,
              left: 10,
              zIndex: 3,
              fontSize: '9.5px',
              padding: '2px 6px',
              background: 'var(--df-wash-badge)',
              border: '1px solid var(--df-brass-edge)',
              color: 'var(--df-brass)',
              borderRadius: '2px',
              pointerEvents: 'none',
            }}
          >
            {phaseBadgeText}
          </div>
        ) : null}
      </div>

      {/* Column 3: Controls (gain, polarity, mute, solo) */}
      <div
        style={{
          width: 'var(--df-lane-ctl-w, 196px)',
          flexShrink: 0,
          padding: '6px 8px',
          display: 'flex',
          alignItems: 'center',
          gap: '10px',
        }}
      >
        {/* Gain slider */}
        <div style={{ flex: 1, minWidth: 0 }}>
          <Slider
            label="gain"
            display={gainDisplay}
            value={effectiveGain}
            min={-30}
            max={12}
            step={0.1}
            bipolar={true}
            onPreview={onGainPreview}
            onCommit={onGainCommit}
          />
        </div>

        {/* Buttons: ø (polarity), M (mute), S (solo) */}
        <div style={{ display: 'flex', gap: '4px' }}>
          <IconButton
            glyph="ø"
            label="Invert polarity"
            active={lane.phase.polarityInverted}
            tone={lane.phase.polarityInverted ? 'accent' : 'default'}
            onClick={onTogglePolarity}
          />
          <IconButton
            glyph="M"
            label={lane.muted ? 'Unmute lane' : 'Mute lane'}
            active={lane.muted}
            tone={lane.muted ? 'destructive' : 'default'}
            onClick={onToggleMute}
          />
          <IconButton
            glyph="S"
            label={lane.soloed ? 'Unsolo lane' : 'Solo lane'}
            active={lane.soloed}
            tone={lane.soloed ? 'accent' : 'default'}
            onClick={onToggleSolo}
          />
        </div>
      </div>
    </div>
  );
}
