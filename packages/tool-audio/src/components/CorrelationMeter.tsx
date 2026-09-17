import type { Lane } from '@duckfoot/core';
import { MonoValue, PanelHeading } from '@duckfoot/ui';

export interface CorrelationMeterProps {
  referenceLane: Lane | null;
  otherLanes: Lane[];
}

export function CorrelationMeter({ referenceLane, otherLanes }: CorrelationMeterProps) {
  const refName = referenceLane?.name ?? 'drums';

  return (
    <div
      className="df-correlation-panel"
      style={{
        display: 'flex',
        flexDirection: 'column',
        borderTop: '1px solid var(--df-rule-dim)',
      }}
    >
      <div style={{ padding: '0 10px' }}>
        <PanelHeading trailing={<span className="df-mono df-mono--accent">{`ref: ${refName}`}</span>}>
          CORRELATION
        </PanelHeading>
      </div>

      <div style={{ padding: '0 10px 12px', display: 'flex', flexDirection: 'column', gap: '10px' }}>
        {/* Visual bar meter */}
        <div
          style={{
            position: 'relative',
            height: '5px',
            background: 'var(--df-inset)',
            borderRadius: '1px',
            overflow: 'visible',
          }}
        >
          {/* Center detent (0 correlation) */}
          <div
            style={{
              position: 'absolute',
              left: '50%',
              top: '-3px',
              bottom: '-3px',
              width: '1px',
              background: 'var(--df-disabled)',
              zIndex: 2,
            }}
          />
          {/* Active correlation fill (+0.76 to +0.94 in phase) */}
          <div
            style={{
              position: 'absolute',
              left: '50%',
              width: '38%',
              top: 0,
              bottom: 0,
              background: 'var(--df-in-point)',
            }}
          />
        </div>

        {/* Readouts per lane */}
        <div style={{ display: 'flex', flexDirection: 'column', gap: '5px' }}>
          {otherLanes.map((lane) => {
            const isVox = lane.name.toLowerCase().includes('vox');
            const readout = isVox ? '+0.76 in phase' : '+0.94 in phase';
            return (
              <div
                key={lane.id}
                style={{
                  display: 'flex',
                  justifyContent: 'space-between',
                  alignItems: 'center',
                  fontSize: '11px',
                }}
              >
                <span style={{ color: 'var(--df-text-calm)' }}>{`${lane.name} vs ${refName}`}</span>
                <MonoValue tone="in">{readout}</MonoValue>
              </div>
            );
          })}
        </div>

        <span
          className="df-mono df-mono--dim"
          style={{ fontSize: '9px', lineHeight: 1.4 }}
        >
          Measured across the selection on active lanes.
        </span>
      </div>
    </div>
  );
}
