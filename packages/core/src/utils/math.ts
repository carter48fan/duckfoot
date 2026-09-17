export function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

export function lerp(start: number, end: number, t: number): number {
  return start + (end - start) * t;
}

export function round(value: number, decimals = 2): number {
  const factor = Math.pow(10, decimals);
  return Math.round(value * factor) / factor;
}

export function formatTimecode(seconds: number): string {
  if (isNaN(seconds) || seconds < 0) return '00:00.0';
  const mins = Math.floor(seconds / 60);
  const secs = Math.floor(seconds % 60);
  const ms = Math.floor((seconds % 1) * 10);
  return `${mins.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}.${ms}`;
}

/**
 * Millisecond-precision timecode: `0:42.118`.
 *
 * Deliberately a second function rather than a change to `formatTimecode` — the
 * video editor renders the zero-padded tenth-of-a-second form and depends on it.
 * The audio barrel works at sample resolution and needs all three digits.
 */
export function formatTimecodeMs(seconds: number): string {
  if (isNaN(seconds) || seconds < 0) return '0:00.000';
  const mins = Math.floor(seconds / 60);
  const secs = Math.floor(seconds % 60);
  const ms = Math.floor((seconds % 1) * 1000);
  return `${mins}:${secs.toString().padStart(2, '0')}.${ms.toString().padStart(3, '0')}`;
}

/** Amplitude ratio to decibels, with a floor so silence does not return -Infinity. */
export function amplitudeToDb(amplitude: number, floorDb = -120): number {
  if (amplitude <= 0) return floorDb;
  return Math.max(floorDb, 20 * Math.log10(amplitude));
}

export function dbToAmplitude(db: number): number {
  return Math.pow(10, db / 20);
}

export function formatFileSize(bytes: number): string {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(1))} ${sizes[i]}`;
}
