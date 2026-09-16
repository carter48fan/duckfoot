/**
 * Harvested and refined from OpenCut Classic.
 * Computes root-mean-square (RMS) waveform peak values from an AudioBuffer.
 */
export function computeRmsWaveform(buffer: AudioBuffer, points: number): number[] {
  const channels = buffer.numberOfChannels;
  const length = buffer.length;
  if (points <= 0 || length === 0) return [];

  const blockSize = Math.floor(length / points);
  const peaks = new Float32Array(points);
  let globalMax = 0;

  for (let c = 0; c < channels; c++) {
    const data = buffer.getChannelData(c);
    for (let i = 0; i < points; i++) {
      const start = i * blockSize;
      const end = Math.min(start + blockSize, length);
      let sum = 0;
      let count = 0;

      for (let j = start; j < end; j += 4) { // step by 4 for fast approximation
        const sample = data[j];
        sum += sample * sample;
        count++;
      }

      const rms = count > 0 ? Math.sqrt(sum / count) : 0;
      if (rms > peaks[i]) {
        peaks[i] = rms;
      }
      if (peaks[i] > globalMax) {
        globalMax = peaks[i];
      }
    }
  }

  const norm = globalMax > 0 ? 1 / globalMax : 1;
  const result: number[] = new Array(points);
  for (let i = 0; i < points; i++) {
    result[i] = Math.min(1, Math.max(0.02, peaks[i] * norm));
  }

  return result;
}
