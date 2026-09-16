/**
 * Audio buffer manipulation primitives: slice, normalize, fade, and WAV export.
 */

export function sliceAudioBuffer(
  audioContext: AudioContext,
  buffer: AudioBuffer,
  startTime: number,
  endTime: number
): AudioBuffer {
  const sampleRate = buffer.sampleRate;
  const startSample = Math.max(0, Math.floor(startTime * sampleRate));
  const endSample = Math.min(buffer.length, Math.ceil(endTime * sampleRate));
  const sliceLength = Math.max(1, endSample - startSample);

  const newBuffer = audioContext.createBuffer(
    buffer.numberOfChannels,
    sliceLength,
    sampleRate
  );

  for (let c = 0; c < buffer.numberOfChannels; c++) {
    const sourceData = buffer.getChannelData(c);
    const targetData = newBuffer.getChannelData(c);
    for (let i = 0; i < sliceLength; i++) {
      targetData[i] = sourceData[startSample + i];
    }
  }

  return newBuffer;
}

export function normalizeAudioBuffer(
  audioContext: AudioContext,
  buffer: AudioBuffer,
  targetPeakDb = -0.1
): AudioBuffer {
  const targetPeak = Math.pow(10, targetPeakDb / 20);
  let maxPeak = 0;

  for (let c = 0; c < buffer.numberOfChannels; c++) {
    const data = buffer.getChannelData(c);
    for (let i = 0; i < data.length; i++) {
      const abs = Math.abs(data[i]);
      if (abs > maxPeak) maxPeak = abs;
    }
  }

  if (maxPeak === 0) return buffer;
  const multiplier = targetPeak / maxPeak;

  const newBuffer = audioContext.createBuffer(
    buffer.numberOfChannels,
    buffer.length,
    buffer.sampleRate
  );

  for (let c = 0; c < buffer.numberOfChannels; c++) {
    const source = buffer.getChannelData(c);
    const target = newBuffer.getChannelData(c);
    for (let i = 0; i < source.length; i++) {
      target[i] = source[i] * multiplier;
    }
  }

  return newBuffer;
}

export function applyFade(
  buffer: AudioBuffer,
  fadeInSeconds: number,
  fadeOutSeconds: number
): void {
  const sampleRate = buffer.sampleRate;
  const fadeInSamples = Math.floor(fadeInSeconds * sampleRate);
  const fadeOutSamples = Math.floor(fadeOutSeconds * sampleRate);
  const totalSamples = buffer.length;

  for (let c = 0; c < buffer.numberOfChannels; c++) {
    const data = buffer.getChannelData(c);

    // Fade in
    if (fadeInSamples > 0) {
      const len = Math.min(fadeInSamples, totalSamples);
      for (let i = 0; i < len; i++) {
        data[i] *= i / len;
      }
    }

    // Fade out
    if (fadeOutSamples > 0) {
      const start = Math.max(0, totalSamples - fadeOutSamples);
      const len = totalSamples - start;
      for (let i = 0; i < len; i++) {
        data[start + i] *= 1 - (i / len);
      }
    }
  }
}

/**
 * Encodes an AudioBuffer into an uncompressed 16-bit PCM WAV Blob.
 */
export function encodeWav(buffer: AudioBuffer): Blob {
  const numChannels = buffer.numberOfChannels;
  const sampleRate = buffer.sampleRate;
  const format = 1; // PCM
  const bitDepth = 16;
  const bytesPerSample = bitDepth / 8;
  const blockAlign = numChannels * bytesPerSample;
  const dataSize = buffer.length * blockAlign;
  const headerSize = 44;
  const arrayBuffer = new ArrayBuffer(headerSize + dataSize);
  const view = new DataView(arrayBuffer);

  // RIFF chunk descriptor
  writeString(view, 0, 'RIFF');
  view.setUint32(4, 36 + dataSize, true);
  writeString(view, 8, 'WAVE');

  // fmt sub-chunk
  writeString(view, 12, 'fmt ');
  view.setUint32(16, 16, true);
  view.setUint16(20, format, true);
  view.setUint16(22, numChannels, true);
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * blockAlign, true);
  view.setUint16(32, blockAlign, true);
  view.setUint16(34, bitDepth, true);

  // data sub-chunk
  writeString(view, 36, 'data');
  view.setUint32(40, dataSize, true);

  // Write PCM samples
  let offset = 44;
  const channels = [];
  for (let c = 0; c < numChannels; c++) {
    channels.push(buffer.getChannelData(c));
  }

  for (let i = 0; i < buffer.length; i++) {
    for (let c = 0; c < numChannels; c++) {
      let sample = channels[c][i];
      sample = Math.max(-1, Math.min(1, sample));
      view.setInt16(offset, sample < 0 ? sample * 0x8000 : sample * 0x7FFF, true);
      offset += 2;
    }
  }

  return new Blob([arrayBuffer], { type: 'audio/wav' });
}

function writeString(view: DataView, offset: number, string: string) {
  for (let i = 0; i < string.length; i++) {
    view.setUint8(offset + i, string.charCodeAt(i));
  }
}
