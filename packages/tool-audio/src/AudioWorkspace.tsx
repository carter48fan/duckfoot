import type { BarrelRegionProps } from '@duckfoot/ui';
import type { EnvelopeNode } from '@duckfoot/core';
import { formatTimecodeMs, orderedLanes } from '@duckfoot/core';
import { useAudio } from './AudioContext';
import { TimeRuler } from './components/TimeRuler';
import { LaneRow } from './components/LaneRow';
import { GainEnvelopeLane } from './components/GainEnvelopeLane';

export function AudioWorkspace({ doc, dispatch }: BarrelRegionProps) {
  const { view } = useAudio();
  const lanes = orderedLanes(doc.audio);

  // Default envelope nodes if none created yet
  const defaultNodes: EnvelopeNode[] = [
    { timeSeconds: doc.audio.selection?.inSeconds ?? 42.118, gainDb: 0.0 },
    { timeSeconds: (doc.audio.selection?.inSeconds ?? 42.118) + 6.822, gainDb: -7.5 },
    { timeSeconds: (doc.audio.selection?.outSeconds ?? 78.226) - 13.926, gainDb: -7.5 },
    { timeSeconds: doc.audio.selection?.outSeconds ?? 78.226, gainDb: 0.0 },
  ];

  const activeEnvelopeNodes =
    view.previewNodes ?? doc.audio.envelope?.nodes ?? defaultNodes;

  const sel = doc.audio.selection;
  const selLength = sel ? Math.max(0, sel.outSeconds - sel.inSeconds) : 0;

  return (
    <div
      className="df-audio-workspace"
      style={{
        display: 'flex',
        flexDirection: 'column',
        height: '100%',
        background: 'var(--df-canvas)',
        overflow: 'hidden',
      }}
    >
      {/* Top info strip */}
      <div
        style={{
          height: 'var(--df-toolbar, 36px)',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          padding: '0 20px',
          borderBottom: '1px solid var(--df-rule)',
          background: 'var(--df-surface)',
          flexShrink: 0,
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
          <span
            style={{
              fontFamily: 'var(--df-font-serif)',
              fontSize: '14px',
              color: 'var(--df-brass-label)',
              letterSpacing: '0.04em',
            }}
          >
            STEM STACK
          </span>
          <span className="df-divider" />
          <span className="df-mono df-mono--dim" style={{ fontSize: '10px' }}>
            {`${lanes.length} lanes · locked at 0:00`}
          </span>
          {sel ? (
            <>
              <span className="df-divider" />
              <span className="df-mono df-mono--in" style={{ fontSize: '10px' }}>
                {`in ${formatTimecodeMs(sel.inSeconds)}`}
              </span>
              <span className="df-mono df-mono--out" style={{ fontSize: '10px' }}>
                {`out ${formatTimecodeMs(sel.outSeconds)}`}
              </span>
              <span className="df-mono df-mono--dim" style={{ fontSize: '10px' }}>
                {`len ${formatTimecodeMs(selLength)}`}
              </span>
            </>
          ) : null}
        </div>

        <div className="df-mono df-mono--dim" style={{ fontSize: '10px' }}>
          48 kHz · 24-bit PCM
        </div>
      </div>

      {/* Stem lanes scroll area */}
      <div
        style={{
          flex: 1,
          overflowY: 'auto',
          display: 'flex',
          flexDirection: 'column',
          padding: '16px 20px',
          gap: '12px',
        }}
      >
        {/* Time ruler */}
        <TimeRuler
          totalDurationSeconds={view.totalDurationSeconds}
          selection={doc.audio.selection}
          playheadSeconds={view.playheadSeconds}
          onSeek={(t) => view.setPlayheadSeconds(t)}
        />

        {lanes.map((lane) => (
          <LaneRow
            key={lane.id}
            lane={lane}
            active={view.activeLaneId === lane.id}
            onSelect={() => view.setActiveLaneId(lane.id)}
            totalDurationSeconds={view.totalDurationSeconds}
            selection={doc.audio.selection}
            playheadSeconds={view.playheadSeconds}
            previewGainDb={view.previewGainDb[lane.id]}
            onGainPreview={(gainDb) =>
              view.setPreviewGainDb({ ...view.previewGainDb, [lane.id]: gainDb })
            }
            onGainCommit={(gainDb) => {
              const next = { ...view.previewGainDb };
              delete next[lane.id];
              view.setPreviewGainDb(next);
              dispatch({ type: 'audio/setLaneGain', laneId: lane.id, gainDb });
            }}
            onToggleMute={() =>
              dispatch({ type: 'audio/setLaneMuted', laneId: lane.id, muted: !lane.muted })
            }
            onToggleSolo={() =>
              dispatch({ type: 'audio/setLaneSoloed', laneId: lane.id, soloed: !lane.soloed })
            }
            onTogglePolarity={() =>
              dispatch({
                type: 'audio/setLanePhase',
                laneId: lane.id,
                patch: { polarityInverted: !lane.phase.polarityInverted },
              })
            }
            onSeek={(t) => view.setPlayheadSeconds(t)}
          />
        ))}

        {/* 4-Node Gain Envelope Lane */}
        <GainEnvelopeLane
          selection={doc.audio.selection}
          totalDurationSeconds={view.totalDurationSeconds}
          nodes={activeEnvelopeNodes}
          interpolation={doc.audio.envelope?.interpolation ?? 'lin'}
          onPreviewNodes={(nodes) => view.setPreviewNodes(nodes)}
          onUpdateNodes={(nodes) => {
            view.setPreviewNodes(null);
            dispatch({ type: 'audio/setEnvelopeNodes', nodes });
          }}
          onChangeInterpolation={(interpolation) =>
            dispatch({ type: 'audio/setEnvelopeInterpolation', interpolation })
          }
          onClear={() => dispatch({ type: 'audio/clearEnvelope' })}
        />

        <div style={{ paddingTop: '8px' }}>
          <span
            className="df-mono df-mono--dim"
            style={{ fontSize: '10px', color: 'var(--df-text-faint, #555)' }}
          >
            the envelope applies to active lanes only · lanes still have no horizontal offset
          </span>
        </div>
      </div>
    </div>
  );
}
