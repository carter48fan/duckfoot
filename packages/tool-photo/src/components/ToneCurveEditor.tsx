import { useRef, useState } from 'react';
import type { CurveNode } from '@duckfoot/core';
import { useDragGesture } from '@duckfoot/ui';
import { buildCurveLut } from '@duckfoot/media';

/**
 * Draggable tone curve.
 *
 * The rendered path comes from the same `buildCurveLut` the pipeline renders
 * through, so the line on screen is the transfer function being applied rather than
 * a decorative spline that happens to pass through the same points.
 */
export function ToneCurveEditor({
  nodes,
  onPreview,
  onCommit,
}: {
  nodes: CurveNode[];
  onPreview: (nodes: CurveNode[]) => void;
  onCommit: (nodes: CurveNode[]) => void;
}) {
  const svgRef = useRef<SVGSVGElement>(null);
  const [dragIndex, setDragIndex] = useState<number | null>(null);

  const lut = buildCurveLut(nodes, 128);
  let path = '';
  for (let i = 0; i < lut.length; i++) {
    const x = (i / (lut.length - 1)) * 100;
    const y = 100 - Math.max(0, Math.min(1, lut[i])) * 100;
    path += `${i === 0 ? 'M' : ' L'} ${x.toFixed(2)} ${y.toFixed(2)}`;
  }

  const pointAt = (event: PointerEvent): CurveNode | null => {
    const svg = svgRef.current;
    if (!svg) return null;
    const rect = svg.getBoundingClientRect();
    return {
      x: Math.max(0, Math.min(1, (event.clientX - rect.left) / rect.width)),
      y: Math.max(0, Math.min(1, 1 - (event.clientY - rect.top) / rect.height)),
    };
  };

  const startDrag = useDragGesture<{ index: number; original: CurveNode[] }>({
    onStart: (event) => {
      const index = Number((event.target as SVGElement).dataset.index ?? -1);
      setDragIndex(index);
      return { index, original: nodes.map((node) => ({ ...node })) };
    },
    onMove: (state, event) => {
      const point = pointAt(event);
      if (!point || state.index < 0) return;
      onPreview(moveNode(state.original, state.index, point));
    },
    onCommit: (state, event) => {
      setDragIndex(null);
      const point = pointAt(event);
      if (!point || state.index < 0) return;
      onCommit(moveNode(state.original, state.index, point));
    },
    onCancel: (state) => {
      setDragIndex(null);
      onPreview(state.original);
    },
  });

  return (
    <svg
      ref={svgRef}
      className="df-tone-curve"
      viewBox="0 0 100 100"
      preserveAspectRatio="none"
      role="application"
      aria-label="Tone curve"
    >
      {[25, 50, 75].map((v) => (
        <g key={v}>
          <line x1={v} y1={0} x2={v} y2={100} stroke="#232323" strokeWidth={0.6} vectorEffect="non-scaling-stroke" />
          <line x1={0} y1={v} x2={100} y2={v} stroke="#232323" strokeWidth={0.6} vectorEffect="non-scaling-stroke" />
        </g>
      ))}
      {/* Identity, for reading how far the curve departs from no-op. */}
      <line x1={0} y1={100} x2={100} y2={0} stroke="#3a3a3a" strokeWidth={0.7} strokeDasharray="3 3" vectorEffect="non-scaling-stroke" />
      <path d={path} fill="none" stroke="var(--df-brass)" strokeWidth={1.6} vectorEffect="non-scaling-stroke" />
      {nodes.map((node, index) => (
        <rect
          key={index}
          data-index={index}
          x={node.x * 100 - 1.6}
          y={100 - node.y * 100 - 1.6}
          width={3.2}
          height={3.2}
          fill={dragIndex === index ? 'var(--df-brass-lit)' : '#f5f5f5'}
          onPointerDown={startDrag}
          style={{ cursor: 'grab' }}
        />
      ))}
    </svg>
  );
}

function moveNode(nodes: CurveNode[], index: number, point: CurveNode): CurveNode[] {
  const next = nodes.map((node) => ({ ...node }));
  // End nodes stay pinned horizontally: a curve whose domain does not span 0..1 has
  // undefined behaviour at the ends, which shows up as clipped blacks or whites.
  const isEnd = index === 0 || index === next.length - 1;
  next[index] = { x: isEnd ? next[index].x : clampBetween(point.x, next, index), y: point.y };
  return next;
}

function clampBetween(x: number, nodes: CurveNode[], index: number): number {
  // Nodes may not cross each other; a non-monotonic domain makes the LUT ambiguous.
  const lower = nodes[index - 1]?.x ?? 0;
  const upper = nodes[index + 1]?.x ?? 1;
  return Math.max(lower + 0.01, Math.min(upper - 0.01, x));
}
