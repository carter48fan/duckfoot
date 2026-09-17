import { useRef } from 'react';
import type { EnvelopeInterpolation, EnvelopeNode, Selection } from '@duckfoot/core';
import { clamp, formatTimecodeMs } from '@duckfoot/core';
import { ChipGroup, ToggleChip, useDragGesture } from '@duckfoot/ui';

export interface GainEnvelopeLaneProps {
  selection: Selection | null;
  totalDurationSeconds: number;
  nodes: EnvelopeNode[];
  interpolation: EnvelopeInterpolation;
  onUpdateNodes: (nodes: EnvelopeNode[]) => void;
  onPreviewNodes: (nodes: EnvelopeNode[]) => void;
  onChangeInterpolation: (interp: EnvelopeInterpolation) => void;
  onClear: () => void;
}

export function GainEnvelopeLane({
  selection,
  totalDurationSeconds,
  nodes,
  interpolation,
  onUpdateNodes,
  onPreviewNodes,
  onChangeInterpolation,
  onClear,
}: GainEnvelopeLaneProps) {
  const canvasRef = useRef<SVGSVGElement>(null);

  // Derive ramp-in and ramp-out durations
  const rampInMs = nodes.length >= 2 ? (nodes[1].timeSeconds - nodes[0].timeSeconds) * 1000 : 0;
  const rampOutMs =
    nodes.length >= 2
      ? (nodes[nodes.length - 1].timeSeconds - nodes[nodes.length - 2].timeSeconds) * 1000
      : 0;

  // Coordinate transforms
  // X: 0 to totalDurationSeconds -> 0% to 100%
  // Y: +6 dB (top = 10%) -> 0 dB (mid = 35%) -> -30 dB (bottom = 90%)
  const minDb = -30;
  const maxDb = 6;
  const dbToPercentY = (db: number) => {
    const clamped = clamp(db, minDb, maxDb);
    const norm = (maxDb - clamped) / (maxDb - minDb);
    return 10 + norm * 80;
  };
  const percentYToDb = (pct: number) => {
    const norm = clamp((pct - 10) / 80, 0, 1);
    return Math.round((maxDb - norm * (maxDb - minDb)) * 10) / 10;
  };

  const timeToPercentX = (t: number) => {
    if (totalDurationSeconds <= 0) return 0;
    return (clamp(t, 0, totalDurationSeconds) / totalDurationSeconds) * 100;
  };
  const percentXToTime = (pct: number) => {
    return (clamp(pct, 0, 100) / 100) * totalDurationSeconds;
  };

  // Node drag gesture
  const startDragNode = (index: number) =>
    // eslint-disable-next-line react-hooks/rules-of-hooks
    useDragGesture<{ initialNodes: EnvelopeNode[] }>({
      onStart: () => ({ initialNodes: [...nodes] }),
      onMove: (_state, e) => {
        const svg = canvasRef.current;
        if (!svg) return;
        const rect = svg.getBoundingClientRect();
        const pctX = ((e.clientX - rect.left) / rect.width) * 100;
        const pctY = ((e.clientY - rect.top) / rect.height) * 100;

        const updated = [...nodes];
        const prevT = index > 0 ? nodes[index - 1].timeSeconds : (selection?.inSeconds ?? 0);
        const nextT =
          index < nodes.length - 1
            ? nodes[index + 1].timeSeconds
            : (selection?.outSeconds ?? totalDurationSeconds);

        const newT = clamp(percentXToTime(pctX), prevT, nextT);
        const newDb = percentYToDb(pctY);

        updated[index] = { timeSeconds: newT, gainDb: newDb };
        onPreviewNodes(updated);
      },
      onCommit: (_state, e) => {
        const svg = canvasRef.current;
        if (!svg) return;
        const rect = svg.getBoundingClientRect();
        const pctX = ((e.clientX - rect.left) / rect.width) * 100;
        const pctY = ((e.clientY - rect.top) / rect.height) * 100;

        const updated = [...nodes];
        const prevT = index > 0 ? nodes[index - 1].timeSeconds : (selection?.inSeconds ?? 0);
        const nextT =
          index < nodes.length - 1
            ? nodes[index + 1].timeSeconds
            : (selection?.outSeconds ?? totalDurationSeconds);

        const newT = clamp(percentXToTime(pctX), prevT, nextT);
        const newDb = percentYToDb(pctY);

        updated[index] = { timeSeconds: newT, gainDb: newDb };
        onUpdateNodes(updated);
      },
      onCancel: (state) => {
        onUpdateNodes(state.initialNodes);
      },
    });

  // Build SVG polyline points
  const pointsString = nodes
    .map((node) => `${timeToPercentX(node.timeSeconds)}%,${dbToPercentY(node.gainDb)}%`)
    .join(' ');

  // Polygon fill under the curve
  const firstX = nodes.length > 0 ? timeToPercentX(nodes[0].timeSeconds) : 0;
  const lastX = nodes.length > 0 ? timeToPercentX(nodes[nodes.length - 1].timeSeconds) : 100;
  const polygonPoints = `${firstX}%,90% ${pointsString} ${lastX}%,90%`;

  return (
    <div
      className="df-gain-envelope-lane"
      style={{
        display: 'flex',
        alignItems: 'stretch',
        height: 'var(--df-envelope-lane, 104px)',
        gap: '12px',
        userSelect: 'none',
      }}
    >
      {/* Column 1: Header and controls */}
      <div
        style={{
          width: 'var(--df-lane-label-w, 110px)',
          flexShrink: 0,
          padding: '6px 8px',
          display: 'flex',
          flexDirection: 'column',
          justifyContent: 'space-between',
        }}
      >
        <div>
          <div
            style={{
              fontSize: '12px',
              fontWeight: 600,
              color: 'var(--df-text)',
              lineHeight: 1.2,
              marginBottom: '2px',
            }}
          >
            gain envelope
          </div>
          <span
            className="df-mono df-mono--dim"
            style={{ fontSize: '9px', display: 'block', marginBottom: '6px' }}
          >
            4 nodes · selection
          </span>
          <ChipGroup>
            {(['lin', 'log', 's-curve'] as const).map((mode) => (
              <ToggleChip
                key={mode}
                label={mode.toUpperCase()}
                active={interpolation === mode}
                onClick={() => onChangeInterpolation(mode)}
              />
            ))}
          </ChipGroup>
        </div>

        <div style={{ display: 'flex', flexDirection: 'column', gap: '2px' }}>
          <span className="df-mono df-mono--dim" style={{ fontSize: '9px' }}>
            {`Ramp in ${Math.round(Math.max(0, rampInMs))} ms`}
          </span>
          <span className="df-mono df-mono--dim" style={{ fontSize: '9px' }}>
            {`Ramp out ${Math.round(Math.max(0, rampOutMs))} ms`}
          </span>
        </div>
      </div>

      {/* Column 2: Envelope graph */}
      <div
        style={{
          flex: 1,
          minWidth: 0,
          position: 'relative',
          background: 'var(--df-well, #101010)',
          border: '1px solid var(--df-rule-soft, #1f1f1f)',
          borderRadius: '2px',
          overflow: 'hidden',
        }}
      >
        {/* Corner dB markers */}
        <span
          className="df-mono"
          style={{
            position: 'absolute',
            top: 4,
            left: 8,
            fontSize: '8.5px',
            color: 'var(--df-text-ghost, #555)',
            pointerEvents: 'none',
            zIndex: 2,
          }}
        >
          +6 dB
        </span>
        <span
          className="df-mono"
          style={{
            position: 'absolute',
            top: `${dbToPercentY(0)}%`,
            left: 8,
            transform: 'translateY(-50%)',
            fontSize: '8.5px',
            color: 'var(--df-text-ghost, #555)',
            pointerEvents: 'none',
            zIndex: 2,
          }}
        >
          0 dB
        </span>
        <span
          className="df-mono"
          style={{
            position: 'absolute',
            bottom: 4,
            left: 8,
            fontSize: '8.5px',
            color: 'var(--df-text-ghost, #555)',
            pointerEvents: 'none',
            zIndex: 2,
          }}
        >
          −30 dB
        </span>

        <svg
          ref={canvasRef}
          style={{
            width: '100%',
            height: '100%',
            display: 'block',
            position: 'absolute',
            inset: 0,
          }}
        >
          {/* 0 dB reference line */}
          <line
            x1="0%"
            y1={`${dbToPercentY(0)}%`}
            x2="100%"
            y2={`${dbToPercentY(0)}%`}
            stroke="var(--df-rule)"
            strokeDasharray="3 3"
          />

          {/* Fill under envelope */}
          {nodes.length >= 2 ? (
            <polygon points={polygonPoints} fill="var(--df-wash-selection-envelope)" />
          ) : null}

          {/* Envelope polyline curve */}
          {nodes.length >= 2 ? (
            <polyline
              points={pointsString}
              fill="none"
              stroke="var(--df-brass)"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          ) : null}

          {/* Interactive node drag handles */}
          {nodes.map((node, i) => {
            const x = `${timeToPercentX(node.timeSeconds)}%`;
            const y = `${dbToPercentY(node.gainDb)}%`;
            // Drag gesture
            const dragHandler = startDragNode(i);

            return (
              <g key={i} onPointerDown={dragHandler} style={{ cursor: 'grab' }}>
                <circle cx={x} cy={y} r={8} fill="transparent" />
                <rect
                  x={`calc(${x} - 4px)`}
                  y={`calc(${y} - 4px)`}
                  width="8"
                  height="8"
                  fill="var(--df-text)"
                  stroke="var(--df-brass)"
                  strokeWidth="1.5"
                />
              </g>
            );
          })}
        </svg>
      </div>

      {/* Column 3: Value table for nodes */}
      <div
        style={{
          width: 'var(--df-lane-ctl-w, 196px)',
          flexShrink: 0,
          padding: '6px 8px',
          display: 'flex',
          flexDirection: 'column',
          justifyContent: 'center',
          gap: '4px',
          fontSize: '10px',
        }}
      >
        {nodes.map((node, i) => (
          <div
            key={i}
            className="df-mono"
            style={{ display: 'flex', justifyContent: 'space-between' }}
          >
            <span style={{ color: 'var(--df-text4)' }}>
              {formatTimecodeMs(node.timeSeconds).slice(0, 8)}
            </span>
            <span style={{ color: node.gainDb === 0 ? 'var(--df-text3)' : 'var(--df-text)' }}>
              {`${node.gainDb > 0 ? '+' : ''}${node.gainDb.toFixed(1)} dB`}
            </span>
          </div>
        ))}
        <button
          type="button"
          onClick={onClear}
          className="df-mono df-mono--dim"
          style={{
            background: 'none',
            border: 'none',
            textAlign: 'right',
            cursor: 'pointer',
            padding: 0,
            marginTop: '2px',
            fontSize: '9px',
            color: 'var(--df-brass)',
          }}
        >
          reset envelope
        </button>
      </div>
    </div>
  );
}
