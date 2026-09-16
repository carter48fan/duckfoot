import React, { useState, useRef, useEffect, useCallback } from 'react';
import {
  type AssetItem,
  formatTimecode,
  VideoExporter
} from '@duckfoot/media';
import { nanoid } from 'nanoid';
import type { TimelineClip, TrackType } from './types';
import {
  Play,
  Pause,
  Scissors,
  Trash2,
  Download,
  Plus,
  Type,
  Video,
  Music,
  Maximize2,
  ZoomIn,
  ZoomOut,
  Layers
} from 'lucide-react';

interface VideoEditorProps {
  assets: AssetItem[];
  onSave?: (blob: Blob, name: string) => void;
}

export function VideoEditor({ assets, onSave }: VideoEditorProps) {
  const [clips, setClips] = useState<TimelineClip[]>([]);
  const [selectedClipId, setSelectedClipId] = useState<string | null>(null);
  const [currentTime, setCurrentTime] = useState<number>(0);
  const [isPlaying, setIsPlaying] = useState<boolean>(false);
  const [timelineZoom, setTimelineZoom] = useState<number>(50); // pixels per second
  const [isExporting, setIsExporting] = useState<boolean>(false);
  const [exportProgress, setExportProgress] = useState<number>(0);

  const canvasRef = useRef<HTMLCanvasElement>(null);
  const videoElementsRef = useRef<Map<string, HTMLVideoElement>>(new Map());
  const audioElementRef = useRef<HTMLAudioElement | null>(null);
  const animFrameRef = useRef<number | null>(null);
  const lastTimeRef = useRef<number>(0);

  // Compute total duration of the timeline
  const totalDuration = Math.max(
    10,
    clips.reduce((acc, clip) => Math.max(acc, clip.startTime + clip.duration), 0)
  );

  // Pre-load video elements for clips
  useEffect(() => {
    clips.forEach((clip) => {
      if (clip.url && (clip.track === 'main' || clip.track === 'overlay') && !clip.textContent) {
        if (!videoElementsRef.current.has(clip.url)) {
          const v = document.createElement('video');
          v.src = clip.url;
          v.crossOrigin = 'anonymous';
          v.preload = 'auto';
          v.muted = true;
          videoElementsRef.current.set(clip.url, v);
        }
      }
    });
  }, [clips]);

  // Render current frame to preview canvas
  const renderFrame = useCallback(
    async (time: number, targetCtx?: CanvasRenderingContext2D) => {
      const canvas = canvasRef.current;
      const ctx = targetCtx || canvas?.getContext('2d');
      if (!ctx || (!canvas && !targetCtx)) return;

      const width = targetCtx ? targetCtx.canvas.width : (canvas?.width || 1280);
      const height = targetCtx ? targetCtx.canvas.height : (canvas?.height || 720);

      // Background
      ctx.fillStyle = '#09090b';
      ctx.fillRect(0, 0, width, height);

      // Render Main Track Clip at current time
      const activeMainClip = clips.find(
        (c) => c.track === 'main' && time >= c.startTime && time < c.startTime + c.duration
      );

      if (activeMainClip && activeMainClip.url) {
        const video = videoElementsRef.current.get(activeMainClip.url);
        if (video) {
          const mediaTime = activeMainClip.trimStart + (time - activeMainClip.startTime);
          if (Math.abs(video.currentTime - mediaTime) > 0.08) {
            video.currentTime = mediaTime;
          }
          try {
            ctx.drawImage(video, 0, 0, width, height);
          } catch {}
        }
      }

      // Render Overlay Track Clips at current time
      const activeOverlays = clips.filter(
        (c) => c.track === 'overlay' && time >= c.startTime && time < c.startTime + c.duration
      );

      for (const overlay of activeOverlays) {
        if (overlay.textContent) {
          ctx.save();
          const fontSize = overlay.fontSize || Math.round(height * 0.06);
          ctx.font = `bold ${fontSize}px sans-serif`;
          ctx.fillStyle = overlay.textColor || '#ffffff';
          ctx.shadowColor = 'rgba(0, 0, 0, 0.8)';
          ctx.shadowBlur = 6;
          ctx.shadowOffsetX = 2;
          ctx.shadowOffsetY = 2;
          ctx.textAlign = 'center';
          ctx.textBaseline = 'middle';
          ctx.fillText(overlay.textContent, width / 2, height * 0.85);
          ctx.restore();
        }
      }
    },
    [clips]
  );

  // Playback Loop
  useEffect(() => {
    if (!isPlaying) {
      if (animFrameRef.current) cancelAnimationFrame(animFrameRef.current);
      return;
    }

    lastTimeRef.current = performance.now();

    const loop = (now: number) => {
      const delta = (now - lastTimeRef.current) / 1000;
      lastTimeRef.current = now;

      setCurrentTime((t) => {
        const next = t + delta;
        if (next >= totalDuration) {
          setIsPlaying(false);
          return 0;
        }
        renderFrame(next);
        return next;
      });

      animFrameRef.current = requestAnimationFrame(loop);
    };

    animFrameRef.current = requestAnimationFrame(loop);
    return () => {
      if (animFrameRef.current) cancelAnimationFrame(animFrameRef.current);
    };
  }, [isPlaying, totalDuration, renderFrame]);

  // Initial and seek frame render
  useEffect(() => {
    if (!isPlaying) {
      renderFrame(currentTime);
    }
  }, [currentTime, isPlaying, renderFrame]);

  // Add Asset to Timeline
  const handleAddAssetToTimeline = (asset: AssetItem) => {
    let track: TrackType = 'main';
    if (asset.type === 'audio') track = 'audio';
    if (asset.type === 'photo') track = 'overlay';

    // Find end of track
    const trackClips = clips.filter((c) => c.track === track);
    const lastClipEnd = trackClips.reduce((acc, c) => Math.max(acc, c.startTime + c.duration), 0);
    const duration = asset.duration || 4; // default 4s for images

    const newClip: TimelineClip = {
      id: nanoid(),
      track,
      name: asset.name,
      assetId: asset.id,
      url: asset.url,
      startTime: lastClipEnd,
      duration: duration,
      trimStart: 0,
      volume: 1,
    };

    setClips((prev) => [...prev, newClip]);
    setSelectedClipId(newClip.id);
  };

  // Add Text Title to Overlay Track
  const handleAddTextOverlay = () => {
    const overlayClips = clips.filter((c) => c.track === 'overlay');
    const lastEnd = overlayClips.reduce((acc, c) => Math.max(acc, c.startTime + c.duration), 0);

    const newClip: TimelineClip = {
      id: nanoid(),
      track: 'overlay',
      name: 'Text Title',
      textContent: 'Enter Your Title Here',
      startTime: Math.max(currentTime, lastEnd),
      duration: 3,
      trimStart: 0,
      fontSize: 48,
      textColor: '#ffffff',
    };

    setClips((prev) => [...prev, newClip]);
    setSelectedClipId(newClip.id);
  };

  // Split Selected Clip at Playhead
  const handleSplitClip = () => {
    if (!selectedClipId) return;
    const clip = clips.find((c) => c.id === selectedClipId);
    if (!clip) return;

    if (currentTime <= clip.startTime || currentTime >= clip.startTime + clip.duration) {
      return; // playhead outside clip bounds
    }

    const splitOffset = currentTime - clip.startTime;
    const leftClip: TimelineClip = {
      ...clip,
      duration: splitOffset,
    };

    const rightClip: TimelineClip = {
      ...clip,
      id: nanoid(),
      startTime: currentTime,
      duration: clip.duration - splitOffset,
      trimStart: clip.trimStart + splitOffset,
    };

    setClips((prev) => prev.map((c) => (c.id === clip.id ? leftClip : c)).concat(rightClip));
    setSelectedClipId(rightClip.id);
  };

  // Delete Selected Clip
  const handleDeleteClip = () => {
    if (!selectedClipId) return;
    setClips((prev) => prev.filter((c) => c.id !== selectedClipId));
    setSelectedClipId(null);
  };

  // Export Timeline to MP4
  const handleExportVideo = async () => {
    setIsExporting(true);
    setExportProgress(0);
    setIsPlaying(false);

    try {
      const exporter = new VideoExporter();
      const blob = await exporter.export({
        width: 1280,
        height: 720,
        fps: 30,
        duration: totalDuration,
        format: 'mp4',
        onProgress: (p) => setExportProgress(Math.round(p * 100)),
        renderFrame: async (t, ctx) => {
          await renderFrame(t, ctx);
        },
      });

      if (onSave) {
        onSave(blob, 'exported-video.mp4');
      } else {
        const url = URL.createObjectURL(blob);
        const a = document.createElement('a');
        a.href = url;
        a.download = 'duckfoot-video.mp4';
        a.click();
        URL.revokeObjectURL(url);
      }
    } catch (err) {
      console.error('Export failed:', err);
    } finally {
      setIsExporting(false);
    }
  };

  return (
    <div className="flex flex-col h-full w-full bg-zinc-950 text-zinc-100 overflow-hidden">
      {/* Top Half: Preview & Inspector */}
      <div className="h-3/5 flex border-b border-zinc-800">
        {/* Preview Screen */}
        <div className="flex-1 flex flex-col items-center justify-center p-4 bg-zinc-900/40 relative">
          <div className="relative aspect-video max-h-full max-w-full rounded-lg overflow-hidden shadow-2xl border border-zinc-800 bg-black flex items-center justify-center">
            <canvas ref={canvasRef} width={1280} height={720} className="w-full h-full object-contain" />
          </div>

          {/* Quick Playback Bar */}
          <div className="mt-3 flex items-center gap-4 bg-zinc-900/90 border border-zinc-800 px-4 py-2 rounded-full backdrop-blur">
            <button
              onClick={() => setIsPlaying(!isPlaying)}
              className="p-2 rounded-full bg-indigo-600 hover:bg-indigo-500 text-white transition active:scale-95"
            >
              {isPlaying ? <Pause className="w-4 h-4" /> : <Play className="w-4 h-4 ml-0.5" />}
            </button>
            <span className="text-xs font-mono text-zinc-300 font-bold min-w-[4rem]">
              {formatTimecode(currentTime)}
            </span>
            <span className="text-xs text-zinc-600">/</span>
            <span className="text-xs font-mono text-zinc-500">
              {formatTimecode(totalDuration)}
            </span>
          </div>
        </div>

        {/* Right Side Inspector */}
        <div className="w-80 border-l border-zinc-800 bg-zinc-900 p-4 flex flex-col justify-between">
          <div className="space-y-4">
            <h3 className="text-xs font-semibold text-zinc-400 uppercase tracking-wider">Clip Properties</h3>

            {selectedClipId ? (
              (() => {
                const clip = clips.find((c) => c.id === selectedClipId);
                if (!clip) return null;
                return (
                  <div className="space-y-4">
                    <div>
                      <label className="text-xs text-zinc-400">Clip Name</label>
                      <input
                        type="text"
                        value={clip.name}
                        onChange={(e) => {
                          const val = e.target.value;
                          setClips((prev) => prev.map((c) => (c.id === clip.id ? { ...c, name: val } : c)));
                        }}
                        className="w-full px-2.5 py-1.5 bg-zinc-800 border border-zinc-700 rounded text-xs text-zinc-100 mt-1"
                      />
                    </div>

                    {clip.textContent !== undefined && (
                      <div>
                        <label className="text-xs text-zinc-400">Text Content</label>
                        <input
                          type="text"
                          value={clip.textContent}
                          onChange={(e) => {
                            const val = e.target.value;
                            setClips((prev) => prev.map((c) => (c.id === clip.id ? { ...c, textContent: val } : c)));
                          }}
                          className="w-full px-2.5 py-1.5 bg-zinc-800 border border-zinc-700 rounded text-xs text-zinc-100 mt-1"
                        />
                      </div>
                    )}

                    <div className="grid grid-cols-2 gap-2 text-xs">
                      <div className="bg-zinc-800/60 p-2 rounded border border-zinc-800">
                        <span className="text-zinc-500 block">Start Time</span>
                        <span className="font-mono text-zinc-200 font-semibold">{formatTimecode(clip.startTime)}</span>
                      </div>
                      <div className="bg-zinc-800/60 p-2 rounded border border-zinc-800">
                        <span className="text-zinc-500 block">Duration</span>
                        <span className="font-mono text-zinc-200 font-semibold">{clip.duration.toFixed(1)}s</span>
                      </div>
                    </div>
                  </div>
                );
              })()
            ) : (
              <div className="text-xs text-zinc-500 italic text-center py-8">
                Select a clip on the timeline below to view and edit its parameters.
              </div>
            )}
          </div>

          <div className="space-y-2 border-t border-zinc-800 pt-4">
            <button
              onClick={handleExportVideo}
              disabled={isExporting}
              className="w-full py-2.5 rounded-lg bg-indigo-600 hover:bg-indigo-500 disabled:bg-zinc-800 text-white text-xs font-semibold flex items-center justify-center gap-2 shadow-lg shadow-indigo-600/20 transition"
            >
              <Download className="w-4 h-4" /> {isExporting ? `Exporting (${exportProgress}%)` : 'Export 1080p MP4'}
            </button>
          </div>
        </div>
      </div>

      {/* Bottom Half: Dedicated 3-Track Timeline */}
      <div className="h-2/5 flex flex-col bg-zinc-950">
        {/* Timeline Header Bar */}
        <div className="h-10 border-b border-zinc-800 px-4 flex items-center justify-between bg-zinc-900/50">
          <div className="flex items-center gap-2">
            <button
              onClick={handleSplitClip}
              disabled={!selectedClipId}
              className="px-2.5 py-1 rounded bg-zinc-800 hover:bg-zinc-700 disabled:opacity-40 text-xs font-medium text-zinc-200 flex items-center gap-1.5 transition"
              title="Split selected clip at playhead (S)"
            >
              <Scissors className="w-3.5 h-3.5" /> Split (S)
            </button>
            <button
              onClick={handleDeleteClip}
              disabled={!selectedClipId}
              className="px-2.5 py-1 rounded bg-zinc-800 hover:bg-zinc-700 disabled:opacity-40 text-xs font-medium text-rose-400 flex items-center gap-1.5 transition"
              title="Delete selected clip"
            >
              <Trash2 className="w-3.5 h-3.5" /> Delete
            </button>
            <div className="h-4 w-px bg-zinc-800 mx-1" />
            <button
              onClick={handleAddTextOverlay}
              className="px-2.5 py-1 rounded bg-zinc-800 hover:bg-zinc-700 text-xs font-medium text-indigo-300 flex items-center gap-1.5 transition"
            >
              <Type className="w-3.5 h-3.5" /> Add Title Overlay
            </button>
          </div>

          <div className="flex items-center gap-2">
            <button
              onClick={() => setTimelineZoom((z) => Math.max(20, z - 10))}
              className="p-1 rounded hover:bg-zinc-800 text-zinc-400"
            >
              <ZoomOut className="w-3.5 h-3.5" />
            </button>
            <button
              onClick={() => setTimelineZoom((z) => Math.min(150, z + 10))}
              className="p-1 rounded hover:bg-zinc-800 text-zinc-400"
            >
              <ZoomIn className="w-3.5 h-3.5" />
            </button>
          </div>
        </div>

        {/* Tracks Area */}
        <div className="flex-1 flex overflow-hidden">
          {/* Left Track Headers */}
          <div className="w-36 border-r border-zinc-800 bg-zinc-900/70 flex flex-col divide-y divide-zinc-800 text-xs font-medium text-zinc-400">
            <div className="h-12 px-3 flex items-center gap-2">
              <Layers className="w-3.5 h-3.5 text-indigo-400" /> Overlay
            </div>
            <div className="h-16 px-3 flex items-center gap-2">
              <Video className="w-3.5 h-3.5 text-emerald-400" /> Main Video
            </div>
            <div className="h-12 px-3 flex items-center gap-2">
              <Music className="w-3.5 h-3.5 text-amber-400" /> Audio Track
            </div>
          </div>

          {/* Right Track Rows & Clips */}
          <div
            className="flex-1 overflow-x-auto relative divide-y divide-zinc-800/60 bg-zinc-950"
            onClick={(e) => {
              const rect = e.currentTarget.getBoundingClientRect();
              const clickX = e.clientX - rect.left + e.currentTarget.scrollLeft;
              const newTime = Math.max(0, clickX / timelineZoom);
              setCurrentTime(newTime);
            }}
          >
            {/* Playhead Vertical Line */}
            <div
              style={{ left: `${currentTime * timelineZoom}px` }}
              className="absolute top-0 bottom-0 w-0.5 bg-red-500 z-20 pointer-events-none"
            >
              <div className="w-2.5 h-2.5 bg-red-500 -ml-1 -top-1 rounded-sm rotate-45" />
            </div>

            {/* Track 1: Overlay */}
            <div className="h-12 relative bg-zinc-900/20">
              {clips
                .filter((c) => c.track === 'overlay')
                .map((clip) => (
                  <div
                    key={clip.id}
                    onClick={(e) => {
                      e.stopPropagation();
                      setSelectedClipId(clip.id);
                    }}
                    style={{
                      left: `${clip.startTime * timelineZoom}px`,
                      width: `${clip.duration * timelineZoom}px`,
                    }}
                    className={`absolute top-1 bottom-1 rounded px-2 flex items-center text-xs font-semibold cursor-pointer border select-none transition ${
                      selectedClipId === clip.id
                        ? 'border-indigo-400 bg-indigo-600/50 text-white ring-2 ring-indigo-500/40'
                        : 'border-indigo-700/50 bg-indigo-950/60 text-indigo-200 hover:bg-indigo-900/60'
                    }`}
                  >
                    <span className="truncate">{clip.name}</span>
                  </div>
                ))}
            </div>

            {/* Track 2: Main Video */}
            <div className="h-16 relative bg-zinc-900/30">
              {clips
                .filter((c) => c.track === 'main')
                .map((clip) => (
                  <div
                    key={clip.id}
                    onClick={(e) => {
                      e.stopPropagation();
                      setSelectedClipId(clip.id);
                    }}
                    style={{
                      left: `${clip.startTime * timelineZoom}px`,
                      width: `${clip.duration * timelineZoom}px`,
                    }}
                    className={`absolute top-1.5 bottom-1.5 rounded px-2.5 flex items-center justify-between text-xs font-semibold cursor-pointer border select-none transition ${
                      selectedClipId === clip.id
                        ? 'border-emerald-400 bg-emerald-600/50 text-white ring-2 ring-emerald-500/40'
                        : 'border-emerald-700/50 bg-emerald-950/60 text-emerald-200 hover:bg-emerald-900/60'
                    }`}
                  >
                    <span className="truncate">{clip.name}</span>
                    <span className="text-[10px] opacity-70 font-mono">{clip.duration.toFixed(1)}s</span>
                  </div>
                ))}
            </div>

            {/* Track 3: Audio */}
            <div className="h-12 relative bg-zinc-900/20">
              {clips
                .filter((c) => c.track === 'audio')
                .map((clip) => (
                  <div
                    key={clip.id}
                    onClick={(e) => {
                      e.stopPropagation();
                      setSelectedClipId(clip.id);
                    }}
                    style={{
                      left: `${clip.startTime * timelineZoom}px`,
                      width: `${clip.duration * timelineZoom}px`,
                    }}
                    className={`absolute top-1 bottom-1 rounded px-2 flex items-center text-xs font-semibold cursor-pointer border select-none transition ${
                      selectedClipId === clip.id
                        ? 'border-amber-400 bg-amber-600/50 text-white ring-2 ring-amber-500/40'
                        : 'border-amber-700/50 bg-amber-950/60 text-amber-200 hover:bg-amber-900/60'
                    }`}
                  >
                    <span className="truncate">{clip.name}</span>
                  </div>
                ))}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
