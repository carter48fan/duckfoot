import { useMemo } from 'react';
import { color } from '@duckfoot/core';
import type { Histogram as HistogramData } from '@duckfoot/media';

/**
 * The RGB histogram, screen-blended.
 *
 * Real, not decorative — computed from the post-pipeline preview scene. It is the
 * cheapest honest scope (about 2 ms over a 1600px preview) and the first thing
 * anyone checks when asking whether a develop pipeline actually ran.
 */
function channelPath(bins: Uint32Array, peak: number): string {
  if (peak <= 0) return 'M 0 100 L 100 100 Z';
  const n = bins.length;
  let d = 'M 0 100';
  let smoothed = 0;
  for (let i = 0; i < n; i++) {
    const x = (i / (n - 1)) * 100;
    // Light smoothing: a raw per-bin histogram of a photograph is spiky enough to
    // read as noise rather than as a distribution.
    smoothed = smoothed * 0.45 + (bins[i] / peak) * 0.55;
    d += ` L ${x.toFixed(2)} ${(100 - Math.min(1, smoothed) * 94).toFixed(2)}`;
  }
  return `${d} L 100 100 Z`;
}

export function Histogram({ data }: { data: HistogramData | null }) {
  const paths = useMemo(() => {
    if (!data) return null;
    const peak = Math.max(
      ...[data.r, data.g, data.b].map((bins) => bins.reduce((m, v) => (v > m ? v : m), 0))
    );
    return [
      { d: channelPath(data.r, peak), stroke: color.scopeR },
      { d: channelPath(data.g, peak), stroke: color.scopeG },
      { d: channelPath(data.b, peak), stroke: color.scopeB },
    ];
  }, [data]);

  return (
    <svg
      className="df-histogram"
      viewBox="0 0 100 100"
      preserveAspectRatio="none"
      role="img"
      aria-label="RGB histogram"
    >
      {[0, 25, 50, 75].map((x) => (
        <line key={x} x1={x} y1={0} x2={x} y2={100} stroke="#242424" strokeWidth={0.6} vectorEffect="non-scaling-stroke" />
      ))}
      {paths?.map((path, index) => (
        <path
          key={index}
          d={path.d}
          fill={path.stroke}
          fillOpacity={0.34}
          stroke={path.stroke}
          strokeOpacity={0.7}
          strokeWidth={0.8}
          vectorEffect="non-scaling-stroke"
        />
      ))}
    </svg>
  );
}
