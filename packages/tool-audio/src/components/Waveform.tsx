import { useEffect, useRef } from 'react';
import { color } from '@duckfoot/core';

export interface WaveformProps {
  peaks?: number[];
  seed?: string;
  muted?: boolean;
  active?: boolean;
  height?: number;
}

/**
 * Deterministic synthetic peaks for preview when buffer decoding is not attached.
 * Generates realistic stem-like transients and envelopes based on the seed.
 */
function generateSyntheticPeaks(seed: string, count: number): number[] {
  let hash = 0;
  for (let i = 0; i < seed.length; i++) {
    hash = (hash << 5) - hash + seed.charCodeAt(i);
    hash |= 0;
  }

  const isDrums = seed.toLowerCase().includes('drum');
  const isBass = seed.toLowerCase().includes('bass');
  const peaks: number[] = new Array(count);

  let currentEnv = 0.3;
  for (let i = 0; i < count; i++) {
    const x = i / count;
    // Multi-frequency rhythm modulation
    const beat = isDrums
      ? Math.pow(Math.sin(x * 120 * Math.PI) * 0.5 + 0.5, 4) * 0.8
      : isBass
        ? (Math.sin(x * 40 * Math.PI) * 0.3 + 0.5) * 0.7
        : (Math.sin(x * 24 * Math.PI) * 0.4 + 0.4) * 0.6;

    // Pseudo-random noise
    hash = (hash * 1664525 + 1013904223) | 0;
    const rnd = ((hash >>> 0) / 4294967296);

    currentEnv = currentEnv * 0.92 + beat * 0.08;
    const val = Math.min(1.0, Math.max(0.04, currentEnv * (0.6 + rnd * 0.6)));
    peaks[i] = val;
  }

  return peaks;
}

export function Waveform({
  peaks,
  seed = 'audio',
  muted = false,
  active = true,
  height = 54,
}: WaveformProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    const rect = canvas.getBoundingClientRect();
    const width = rect.width || 400;
    const actualHeight = rect.height || height;

    canvas.width = Math.floor(width * dpr);
    canvas.height = Math.floor(actualHeight * dpr);

    ctx.scale(dpr, dpr);
    ctx.clearRect(0, 0, width, actualHeight);

    const barWidth = 2;
    const gap = 1;
    const step = barWidth + gap;
    const barCount = Math.floor(width / step);

    const data = peaks && peaks.length > 0 ? peaks : generateSyntheticPeaks(seed, barCount);
    const midY = actualHeight / 2;
    const ink = muted ? color.waveDim : active ? color.wave : color.waveDim;

    ctx.fillStyle = ink;

    for (let i = 0; i < barCount; i++) {
      const dataIdx = Math.floor((i / barCount) * data.length);
      const amp = data[dataIdx] ?? 0.1;
      const barH = Math.max(2, amp * (actualHeight - 6));
      const x = i * step;
      const y = midY - barH / 2;

      ctx.fillRect(x, y, barWidth, barH);
    }
  }, [peaks, seed, muted, active, height]);

  return (
    <canvas
      ref={canvasRef}
      style={{
        width: '100%',
        height: `${height}px`,
        display: 'block',
      }}
    />
  );
}
