import type { Selection } from '@duckfoot/core';

export interface SelectionOverlayProps {
  selection: Selection | null;
  totalDurationSeconds: number;
  showLabels?: boolean;
}

/**
 * The single component rendering the shared selection across all lanes, the ruler,
 * and the gain envelope lane.
 *
 * DESIGN.md: "green = in-point, red = out-point, accent wash = selection.
 * in/out spans ALL lanes."
 */
export function SelectionOverlay({
  selection,
  totalDurationSeconds,
  showLabels = false,
}: SelectionOverlayProps) {
  if (!selection || totalDurationSeconds <= 0) return null;

  const leftPercent = Math.max(0, Math.min(100, (selection.inSeconds / totalDurationSeconds) * 100));
  const rightPercent = Math.max(0, Math.min(100, (selection.outSeconds / totalDurationSeconds) * 100));
  const widthPercent = Math.max(0, rightPercent - leftPercent);

  return (
    <div
      className="df-selection-overlay"
      style={{
        position: 'absolute',
        top: 0,
        bottom: 0,
        left: `${leftPercent}%`,
        width: `${widthPercent}%`,
        background: 'var(--df-wash-selection-lane)',
        borderLeft: '2px solid var(--df-in-point)',
        borderRight: '2px solid var(--df-out-point)',
        pointerEvents: 'none',
        zIndex: 2,
      }}
    >
      {showLabels ? (
        <>
          <span
            className="df-mono"
            style={{
              position: 'absolute',
              top: 2,
              left: 4,
              fontSize: '8.5px',
              fontWeight: 600,
              color: 'var(--df-in-point)',
              letterSpacing: '0.05em',
            }}
          >
            IN
          </span>
          <span
            className="df-mono"
            style={{
              position: 'absolute',
              top: 2,
              right: 4,
              fontSize: '8.5px',
              fontWeight: 600,
              color: 'var(--df-out-point)',
              letterSpacing: '0.05em',
            }}
          >
            OUT
          </span>
        </>
      ) : null}
    </div>
  );
}
