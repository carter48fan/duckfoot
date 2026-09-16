import React, { useState, useRef, useEffect, useCallback } from 'react';
import {
  type AssetItem,
  formatTimecode,
  computeRmsWaveform,
  sliceAudioBuffer,
  normalizeAudioBuffer,
  applyFade,
  encodeWav
} from '@opencut/media';
import {
  Play,
  Pause,
  Scissors,
  Download,
  RotateCcw,
  Volume2,
  VolumeX,
  Sparkles,
  ArrowRightToLine,
  ArrowLeftToLine,
  Music
} from 'lucide-react';

interface AudioCutterProps {
  asset?: AssetItem | null;
  onSave?: (blob: Blob, name: string) => void;
}

export function AudioCutter({ asset, onSave }: AudioCutterProps) {
  const [audioBuffer, setAudioBuffer] = useState<AudioBuffer | null>(null);
  const [originalBuffer, setOriginalBuffer] = useState<AudioBuffer | null>(null);
  const [waveformPeaks, setWaveformPeaks] = useState<number[]>([]);
  const [isPlaying, setIsPlaying] = useState<boolean>(false);
  const [currentTime, setCurrentTime] = useState<number>(0);
  const [inPoint, setInPoint] = useState<number>(0);
  const [outPoint, setOutPoint] = useState<number>(0);
  const [volume, setVolume] = useState<number>(1);
  const [isMuted, setIsMuted] = useState<boolean>(false);
  const [isLoading, setIsLoading] = useState<boolean>(false);

  const canvasRef = useRef<HTMLCanvasElement>(null);
  const audioContextRef = useRef<AudioContext | null>(null);
  const sourceNodeRef = useRef<AudioBufferSourceNode | null>(null);
  const gainNodeRef = useRef<GainNode | null>(null);
  const startTimeRef = useRef<number>(0);
  const pauseTimeRef = useRef<number>(0);
  const animFrameRef = useRef<number | null>(null);

  // Initialize AudioContext
  useEffect(() => {
    const AudioCtx = window.AudioContext || (window as any).webkitAudioContext;
    audioContextRef.current = new AudioCtx();
    return () => {
      if (audioContextRef.current && audioContextRef.current.state !== 'closed') {
        audioContextRef.current.close();
      }
    };
  }, []);

  // Load and decode audio from asset
  useEffect(() => {
    if (!asset || (asset.type !== 'audio' && asset.type !== 'video')) {
      setAudioBuffer(null);
      setOriginalBuffer(null);
      setWaveformPeaks([]);
      return;
    }

    let isCancelled = false;
    setIsLoading(true);

    const loadAudio = async () => {
      try {
        const response = await fetch(asset.url);
        const arrayBuffer = await response.arrayBuffer();
        if (isCancelled || !audioContextRef.current) return;

        const decoded = await audioContextRef.current.decodeAudioData(arrayBuffer);
        if (isCancelled) return;

        setOriginalBuffer(decoded);
        setAudioBuffer(decoded);
        setInPoint(0);
        setOutPoint(decoded.duration);
        setCurrentTime(0);
        pauseTimeRef.current = 0;

        // Generate 300 RMS peak bars
        const peaks = computeRmsWaveform(decoded, 300);
        setWaveformPeaks(peaks);
      } catch (err) {
        console.error('Failed to decode audio file:', err);
      } finally {
        if (!isCancelled) setIsLoading(false);
      }
    };

    loadAudio();
    return () => {
      isCancelled = true;
      stopPlayback();
    };
  }, [asset]);

  const stopPlayback = useCallback(() => {
    if (sourceNodeRef.current) {
      try {
        sourceNodeRef.current.stop();
        sourceNodeRef.current.disconnect();
      } catch {}
      sourceNodeRef.current = null;
    }
    if (animFrameRef.current) {
      cancelAnimationFrame(animFrameRef.current);
      animFrameRef.current = null;
    }
    setIsPlaying(false);
  }, []);

  const startPlayback = (fromTime: number) => {
    const ctx = audioContextRef.current;
    if (!ctx || !audioBuffer) return;

    stopPlayback();
    if (ctx.state === 'suspended') ctx.resume();

    const source = ctx.createBufferSource();
    source.buffer = audioBuffer;

    const gain = ctx.createGain();
    gain.gain.value = isMuted ? 0 : volume;
    gainNodeRef.current = gain;

    source.connect(gain);
    gain.connect(ctx.destination);

    const offset = Math.max(0, Math.min(fromTime, audioBuffer.duration));
    source.start(0, offset);
    sourceNodeRef.current = source;
    startTimeRef.current = ctx.currentTime - offset;
    setIsPlaying(true);

    source.onended = () => {
      setIsPlaying(false);
      setCurrentTime(audioBuffer.duration);
      pauseTimeRef.current = audioBuffer.duration;
    };

    const updatePlayhead = () => {
      if (!ctx || !sourceNodeRef.current) return;
      const elapsed = ctx.currentTime - startTimeRef.current;
      if (elapsed >= audioBuffer.duration) {
        setCurrentTime(audioBuffer.duration);
        setIsPlaying(false);
        return;
      }
      setCurrentTime(elapsed);
      pauseTimeRef.current = elapsed;
      animFrameRef.current = requestAnimationFrame(updatePlayhead);
    };
    animFrameRef.current = requestAnimationFrame(updatePlayhead);
  };

  const togglePlayPause = () => {
    if (isPlaying) {
      pauseTimeRef.current = currentTime;
      stopPlayback();
    } else {
      const seekPos = currentTime >= (audioBuffer?.duration || 0) ? inPoint : currentTime;
      startPlayback(seekPos);
    }
  };

  // Keyboard shortcut: Space to toggle play/pause
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.code === 'Space' && e.target === document.body) {
        e.preventDefault();
        togglePlayPause();
      } else if (e.key === 'i' || e.key === 'I') {
        setInPoint(currentTime);
      } else if (e.key === 'o' || e.key === 'O') {
        setOutPoint(currentTime);
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [isPlaying, currentTime, inPoint, outPoint]);

  // Draw Waveform Canvas
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || waveformPeaks.length === 0 || !audioBuffer) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const width = canvas.parentElement?.clientWidth || 800;
    const height = 180;
    canvas.width = width;
    canvas.height = height;

    ctx.clearRect(0, 0, width, height);

    const barCount = waveformPeaks.length;
    const barWidth = width / barCount;
    const duration = audioBuffer.duration;

    const inX = (inPoint / duration) * width;
    const outX = (outPoint / duration) * width;
    const playheadX = (currentTime / duration) * width;

    // Draw In/Out Highlight Background
    ctx.fillStyle = 'rgba(99, 102, 241, 0.15)';
    ctx.fillRect(inX, 0, Math.max(0, outX - inX), height);

    // Draw Waveform Bars
    for (let i = 0; i < barCount; i++) {
      const peak = waveformPeaks[i];
      const barHeight = Math.max(4, peak * (height - 20));
      const x = i * barWidth;
      const y = (height - barHeight) / 2;

      // Color active range vs outside range
      const isInsideSelection = x >= inX && x <= outX;
      ctx.fillStyle = isInsideSelection ? '#818cf8' : '#52525b';

      ctx.beginPath();
      ctx.roundRect(x, y, Math.max(1, barWidth - 1), barHeight, 2);
      ctx.fill();
    }

    // Draw In Marker Line
    ctx.strokeStyle = '#22c55e';
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.moveTo(inX, 0);
    ctx.lineTo(inX, height);
    ctx.stroke();

    // Draw Out Marker Line
    ctx.strokeStyle = '#ef4444';
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.moveTo(outX, 0);
    ctx.lineTo(outX, height);
    ctx.stroke();

    // Draw Playhead Line
    ctx.strokeStyle = '#ffffff';
    ctx.lineWidth = 2;
    ctx.shadowColor = 'rgba(0, 0, 0, 0.8)';
    ctx.shadowBlur = 4;
    ctx.beginPath();
    ctx.moveTo(playheadX, 0);
    ctx.lineTo(playheadX, height);
    ctx.stroke();
  }, [waveformPeaks, audioBuffer, currentTime, inPoint, outPoint]);

  // Waveform Click Seeking
  const handleWaveformClick = (e: React.MouseEvent<HTMLCanvasElement>) => {
    const canvas = canvasRef.current;
    if (!canvas || !audioBuffer) return;
    const rect = canvas.getBoundingClientRect();
    const clickX = e.clientX - rect.left;
    const ratio = Math.max(0, Math.min(1, clickX / rect.width));
    const newTime = ratio * audioBuffer.duration;
    setCurrentTime(newTime);
    pauseTimeRef.current = newTime;
    if (isPlaying) {
      startPlayback(newTime);
    }
  };

  // Trim to In/Out selection
  const handleTrimToSelection = () => {
    const ctx = audioContextRef.current;
    if (!ctx || !audioBuffer) return;
    stopPlayback();

    const start = Math.min(inPoint, outPoint);
    const end = Math.max(inPoint, outPoint);
    const sliced = sliceAudioBuffer(ctx, audioBuffer, start, end);

    setAudioBuffer(sliced);
    setInPoint(0);
    setOutPoint(sliced.duration);
    setCurrentTime(0);
    pauseTimeRef.current = 0;
    setWaveformPeaks(computeRmsWaveform(sliced, 300));
  };

  // Delete selection
  const handleDeleteSelection = () => {
    const ctx = audioContextRef.current;
    if (!ctx || !audioBuffer) return;
    stopPlayback();

    const start = Math.min(inPoint, outPoint);
    const end = Math.max(inPoint, outPoint);
    if (start <= 0 && end >= audioBuffer.duration) return;

    // Slice left portion
    const leftBuffer = start > 0 ? sliceAudioBuffer(ctx, audioBuffer, 0, start) : null;
    // Slice right portion
    const rightBuffer = end < audioBuffer.duration ? sliceAudioBuffer(ctx, audioBuffer, end, audioBuffer.duration) : null;

    const totalSamples = (leftBuffer?.length || 0) + (rightBuffer?.length || 0);
    const newBuffer = ctx.createBuffer(audioBuffer.numberOfChannels, totalSamples, audioBuffer.sampleRate);

    for (let c = 0; c < audioBuffer.numberOfChannels; c++) {
      const target = newBuffer.getChannelData(c);
      let offset = 0;
      if (leftBuffer) {
        target.set(leftBuffer.getChannelData(c), 0);
        offset += leftBuffer.length;
      }
      if (rightBuffer) {
        target.set(rightBuffer.getChannelData(c), offset);
      }
    }

    setAudioBuffer(newBuffer);
    setInPoint(0);
    setOutPoint(newBuffer.duration);
    setCurrentTime(0);
    pauseTimeRef.current = 0;
    setWaveformPeaks(computeRmsWaveform(newBuffer, 300));
  };

  // Normalize audio
  const handleNormalize = () => {
    const ctx = audioContextRef.current;
    if (!ctx || !audioBuffer) return;
    const normalized = normalizeAudioBuffer(ctx, audioBuffer, -0.1);
    setAudioBuffer(normalized);
    setWaveformPeaks(computeRmsWaveform(normalized, 300));
  };

  // Apply Fades
  const handleApplyFade = (type: 'in' | 'out') => {
    if (!audioBuffer) return;
    if (type === 'in') applyFade(audioBuffer, 1.5, 0);
    if (type === 'out') applyFade(audioBuffer, 0, 1.5);
    setWaveformPeaks(computeRmsWaveform(audioBuffer, 300));
  };

  // Reset to original
  const handleReset = () => {
    if (!originalBuffer) return;
    stopPlayback();
    setAudioBuffer(originalBuffer);
    setInPoint(0);
    setOutPoint(originalBuffer.duration);
    setCurrentTime(0);
    pauseTimeRef.current = 0;
    setWaveformPeaks(computeRmsWaveform(originalBuffer, 300));
  };

  // Export WAV
  const handleExportWav = () => {
    if (!audioBuffer) return;
    const wavBlob = encodeWav(audioBuffer);
    const name = (asset?.name?.replace(/\.[^/.]+$/, '') || 'edited-audio') + '.wav';

    if (onSave) {
      onSave(wavBlob, name);
    } else {
      const url = URL.createObjectURL(wavBlob);
      const a = document.createElement('a');
      a.href = url;
      a.download = name;
      a.click();
      URL.revokeObjectURL(url);
    }
  };

  if (!asset || (!audioBuffer && !isLoading)) {
    return (
      <div className="flex flex-col items-center justify-center h-full text-zinc-400 bg-zinc-950 p-8">
        <div className="p-6 rounded-2xl bg-zinc-900 border border-zinc-800 text-center max-w-sm">
          <Music className="w-12 h-12 mx-auto text-zinc-500 mb-4" />
          <h3 className="text-lg font-semibold text-zinc-200 mb-2">No Audio Track Selected</h3>
          <p className="text-sm text-zinc-400">
            Select an audio or video file from the Asset Shelf on the left to inspect and cut its waveform.
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full w-full bg-zinc-950 text-zinc-100 p-6 overflow-hidden">
      {/* Top Header */}
      <div className="flex items-center justify-between pb-4 border-b border-zinc-800">
        <div>
          <h2 className="text-lg font-bold text-zinc-100">{asset.name}</h2>
          <div className="flex items-center gap-3 text-xs text-zinc-400 mt-0.5">
            <span>Duration: <strong className="text-zinc-200">{formatTimecode(audioBuffer?.duration || 0)}</strong></span>
            <span>•</span>
            <span>Sample Rate: <strong className="text-zinc-200">{audioBuffer?.sampleRate} Hz</strong></span>
            <span>•</span>
            <span>Channels: <strong className="text-zinc-200">{audioBuffer?.numberOfChannels === 2 ? 'Stereo' : 'Mono'}</strong></span>
          </div>
        </div>

        <div className="flex items-center gap-2">
          <button
            onClick={handleReset}
            className="px-3 py-1.5 rounded bg-zinc-800 hover:bg-zinc-700 text-xs font-medium text-zinc-300 hover:text-white flex items-center gap-1.5 transition"
          >
            <RotateCcw className="w-3.5 h-3.5" /> Revert Original
          </button>
          <button
            onClick={handleExportWav}
            className="px-4 py-1.5 rounded bg-indigo-600 hover:bg-indigo-500 text-xs font-semibold text-white flex items-center gap-1.5 shadow-lg shadow-indigo-500/20 transition"
          >
            <Download className="w-4 h-4" /> Export WAV
          </button>
        </div>
      </div>

      {/* Main Waveform Display */}
      <div className="flex-1 flex flex-col justify-center py-6">
        <div className="relative rounded-xl bg-zinc-900 border border-zinc-800 p-4 shadow-inner">
          <canvas
            ref={canvasRef}
            onClick={handleWaveformClick}
            className="w-full h-44 cursor-crosshair block rounded"
          />

          {/* Time markers bar */}
          <div className="flex justify-between items-center text-xs font-mono text-zinc-500 pt-2 border-t border-zinc-800/80 mt-2">
            <span>00:00.0</span>
            <span className="text-indigo-400 font-bold">{formatTimecode(currentTime)}</span>
            <span>{formatTimecode(audioBuffer?.duration || 0)}</span>
          </div>
        </div>
      </div>

      {/* Control Console */}
      <div className="bg-zinc-900 border border-zinc-800 rounded-xl p-4 flex flex-wrap items-center justify-between gap-4">
        {/* Playback Controls */}
        <div className="flex items-center gap-3">
          <button
            onClick={togglePlayPause}
            className="w-12 h-12 rounded-full bg-indigo-600 hover:bg-indigo-500 text-white flex items-center justify-center shadow-lg shadow-indigo-600/30 transition active:scale-95"
            title="Play / Pause (Space)"
          >
            {isPlaying ? <Pause className="w-5 h-5" /> : <Play className="w-5 h-5 ml-0.5" />}
          </button>

          <div className="flex flex-col">
            <span className="text-xs text-zinc-400 font-medium">Position</span>
            <span className="text-sm font-mono text-zinc-100 font-bold">{formatTimecode(currentTime)}</span>
          </div>

          <div className="h-6 w-px bg-zinc-800 mx-1" />

          {/* Range In / Out Buttons */}
          <div className="flex items-center gap-1.5">
            <button
              onClick={() => setInPoint(currentTime)}
              className="px-2.5 py-1.5 rounded bg-zinc-800 hover:bg-zinc-700 text-xs font-medium text-emerald-400 flex items-center gap-1 border border-emerald-500/20"
              title="Set In Point (I)"
            >
              <ArrowRightToLine className="w-3.5 h-3.5" /> In: {formatTimecode(inPoint)}
            </button>
            <button
              onClick={() => setOutPoint(currentTime)}
              className="px-2.5 py-1.5 rounded bg-zinc-800 hover:bg-zinc-700 text-xs font-medium text-rose-400 flex items-center gap-1 border border-rose-500/20"
              title="Set Out Point (O)"
            >
              <ArrowLeftToLine className="w-3.5 h-3.5" /> Out: {formatTimecode(outPoint)}
            </button>
          </div>
        </div>

        {/* Editing Actions */}
        <div className="flex items-center gap-2">
          <button
            onClick={handleTrimToSelection}
            className="px-3 py-1.5 rounded bg-zinc-800 hover:bg-zinc-700 text-xs font-medium text-zinc-200 hover:text-white flex items-center gap-1.5 border border-zinc-700 transition"
            title="Crop audio down to In-Out selection"
          >
            <Scissors className="w-3.5 h-3.5 text-indigo-400" /> Trim Selection
          </button>
          <button
            onClick={handleDeleteSelection}
            className="px-3 py-1.5 rounded bg-zinc-800 hover:bg-zinc-700 text-xs font-medium text-zinc-200 hover:text-white flex items-center gap-1.5 border border-zinc-700 transition"
            title="Delete the In-Out region"
          >
            Delete Region
          </button>
          <button
            onClick={handleNormalize}
            className="px-3 py-1.5 rounded bg-zinc-800 hover:bg-zinc-700 text-xs font-medium text-amber-300 hover:text-amber-200 flex items-center gap-1.5 border border-amber-500/20 transition"
            title="Normalize peak amplitude to -0.1 dB"
          >
            <Sparkles className="w-3.5 h-3.5" /> Normalize Peak
          </button>
          <button
            onClick={() => handleApplyFade('in')}
            className="px-2.5 py-1.5 rounded bg-zinc-800 hover:bg-zinc-700 text-xs font-medium text-zinc-300 hover:text-white border border-zinc-700 transition"
          >
            Fade In (1.5s)
          </button>
          <button
            onClick={() => handleApplyFade('out')}
            className="px-2.5 py-1.5 rounded bg-zinc-800 hover:bg-zinc-700 text-xs font-medium text-zinc-300 hover:text-white border border-zinc-700 transition"
          >
            Fade Out (1.5s)
          </button>
        </div>

        {/* Volume & Monitoring */}
        <div className="flex items-center gap-2">
          <button
            onClick={() => setIsMuted(!isMuted)}
            className="p-2 rounded hover:bg-zinc-800 text-zinc-400 hover:text-white"
          >
            {isMuted ? <VolumeX className="w-4 h-4 text-rose-400" /> : <Volume2 className="w-4 h-4" />}
          </button>
          <input
            type="range"
            min="0"
            max="1.5"
            step="0.05"
            value={isMuted ? 0 : volume}
            onChange={(e) => {
              setVolume(Number(e.target.value));
              setIsMuted(false);
              if (gainNodeRef.current) gainNodeRef.current.gain.value = Number(e.target.value);
            }}
            className="w-20 h-1.5 bg-zinc-700 rounded-lg appearance-none cursor-pointer accent-indigo-500"
          />
          <span className="text-xs font-mono text-zinc-400 w-8">{Math.round(volume * 100)}%</span>
        </div>
      </div>
    </div>
  );
}
