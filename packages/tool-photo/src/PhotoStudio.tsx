import React, { useState, useRef, useEffect, useCallback } from 'react';
import {
  type ImageAdjustments,
  DEFAULT_ADJUSTMENTS,
  applyImageAdjustments,
  cropCanvas,
  type AssetItem
} from '@duckfoot/media';
import {
  Crop,
  RotateCw,
  FlipHorizontal,
  FlipVertical,
  Sliders,
  Download,
  RotateCcw,
  Type,
  ZoomIn,
  ZoomOut,
  Maximize2
} from 'lucide-react';

interface PhotoStudioProps {
  asset?: AssetItem | null;
  onSave?: (blob: Blob, name: string) => void;
}

export function PhotoStudio({ asset, onSave }: PhotoStudioProps) {
  const [image, setImage] = useState<HTMLImageElement | null>(null);
  const [adjustments, setAdjustments] = useState<ImageAdjustments>({ ...DEFAULT_ADJUSTMENTS });
  const [rotation, setRotation] = useState<number>(0);
  const [flipH, setFlipH] = useState<boolean>(false);
  const [flipV, setFlipV] = useState<boolean>(false);
  const [watermark, setWatermark] = useState<string>('');
  const [cropAspect, setCropAspect] = useState<string>('free'); // 'free', '1:1', '4:5', '16:9', '9:16'
  const [isCropping, setIsCropping] = useState<boolean>(false);
  const [zoom, setZoom] = useState<number>(1);
  const [activeTab, setActiveTab] = useState<'adjust' | 'crop' | 'text'>('adjust');

  const canvasRef = useRef<HTMLCanvasElement>(null);

  // Load image when asset changes
  useEffect(() => {
    if (!asset || asset.type !== 'photo') {
      setImage(null);
      return;
    }
    const img = new Image();
    img.crossOrigin = 'anonymous';
    img.src = asset.url;
    img.onload = () => {
      setImage(img);
      setAdjustments({ ...DEFAULT_ADJUSTMENTS });
      setRotation(0);
      setFlipH(false);
      setFlipV(false);
      setZoom(1);
    };
  }, [asset]);

  // Render canvas
  const renderCanvas = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas || !image) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const isRotated90or270 = rotation % 180 !== 0;
    const width = isRotated90or270 ? image.height : image.width;
    const height = isRotated90or270 ? image.width : image.height;

    canvas.width = width;
    canvas.height = height;

    ctx.save();
    ctx.clearRect(0, 0, width, height);

    // Apply orientation transforms
    ctx.translate(width / 2, height / 2);
    ctx.rotate((rotation * Math.PI) / 180);
    ctx.scale(flipH ? -1 : 1, flipV ? -1 : 1);
    ctx.drawImage(image, -image.width / 2, -image.height / 2);
    ctx.restore();

    // Apply color and exposure adjustments
    applyImageAdjustments(ctx, width, height, adjustments);

    // Render watermark if present
    if (watermark.trim()) {
      ctx.save();
      const fontSize = Math.max(16, Math.round(width * 0.035));
      ctx.font = `bold ${fontSize}px sans-serif`;
      ctx.fillStyle = 'rgba(255, 255, 255, 0.85)';
      ctx.shadowColor = 'rgba(0, 0, 0, 0.6)';
      ctx.shadowBlur = 8;
      ctx.shadowOffsetX = 2;
      ctx.shadowOffsetY = 2;
      ctx.textAlign = 'right';
      ctx.textBaseline = 'bottom';
      ctx.fillText(watermark, width - 24, height - 24);
      ctx.restore();
    }
  }, [image, adjustments, rotation, flipH, flipV, watermark]);

  useEffect(() => {
    renderCanvas();
  }, [renderCanvas]);

  const handleExport = (format: 'png' | 'jpeg' | 'webp') => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const mime = `image/${format}`;
    canvas.toBlob(
      (blob) => {
        if (!blob) return;
        const name = (asset?.name?.replace(/\.[^/.]+$/, '') || 'edited-photo') + `.${format}`;
        if (onSave) {
          onSave(blob, name);
        } else {
          const url = URL.createObjectURL(blob);
          const a = document.createElement('a');
          a.href = url;
          a.download = name;
          a.click();
          URL.revokeObjectURL(url);
        }
      },
      mime,
      0.95
    );
  };

  const handleRotate = () => {
    setRotation((prev) => (prev + 90) % 360);
  };

  const handleResetAdjustments = () => {
    setAdjustments({ ...DEFAULT_ADJUSTMENTS });
  };

  if (!asset || !image) {
    return (
      <div className="flex flex-col items-center justify-center h-full text-zinc-400 bg-zinc-950 p-8">
        <div className="p-6 rounded-2xl bg-zinc-900 border border-zinc-800 text-center max-w-sm">
          <Sliders className="w-12 h-12 mx-auto text-zinc-500 mb-4" />
          <h3 className="text-lg font-semibold text-zinc-200 mb-2">No Photo Selected</h3>
          <p className="text-sm text-zinc-400">
            Select or import a photo from the Asset Shelf on the left to start editing.
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="flex h-full w-full bg-zinc-950 text-zinc-100 overflow-hidden">
      {/* Central Canvas Viewport */}
      <div className="flex-1 flex flex-col relative overflow-hidden bg-zinc-900/50">
        {/* Top Viewport Toolbar */}
        <div className="h-12 border-b border-zinc-800 px-4 flex items-center justify-between bg-zinc-900/80 backdrop-blur z-10">
          <div className="flex items-center space-x-2">
            <button
              onClick={handleRotate}
              className="p-1.5 rounded hover:bg-zinc-800 text-zinc-300 hover:text-white"
              title="Rotate 90° Clockwise"
            >
              <RotateCw className="w-4 h-4" />
            </button>
            <button
              onClick={() => setFlipH((prev) => !prev)}
              className="p-1.5 rounded hover:bg-zinc-800 text-zinc-300 hover:text-white"
              title="Flip Horizontally"
            >
              <FlipHorizontal className="w-4 h-4" />
            </button>
            <button
              onClick={() => setFlipV((prev) => !prev)}
              className="p-1.5 rounded hover:bg-zinc-800 text-zinc-300 hover:text-white"
              title="Flip Vertically"
            >
              <FlipVertical className="w-4 h-4" />
            </button>
            <div className="h-4 w-px bg-zinc-800 mx-1" />
            <span className="text-xs text-zinc-400 font-mono">
              {image.naturalWidth} × {image.naturalHeight}px
            </span>
          </div>

          <div className="flex items-center space-x-2">
            <button
              onClick={() => setZoom((z) => Math.max(0.2, z - 0.1))}
              className="p-1.5 rounded hover:bg-zinc-800 text-zinc-300 hover:text-white"
              title="Zoom Out"
            >
              <ZoomOut className="w-4 h-4" />
            </button>
            <span className="text-xs text-zinc-400 font-mono min-w-[3rem] text-center">
              {Math.round(zoom * 100)}%
            </span>
            <button
              onClick={() => setZoom((z) => Math.min(3, z + 0.1))}
              className="p-1.5 rounded hover:bg-zinc-800 text-zinc-300 hover:text-white"
              title="Zoom In"
            >
              <ZoomIn className="w-4 h-4" />
            </button>
            <button
              onClick={() => setZoom(1)}
              className="p-1.5 rounded hover:bg-zinc-800 text-zinc-300 hover:text-white"
              title="Fit to Screen"
            >
              <Maximize2 className="w-4 h-4" />
            </button>
          </div>
        </div>

        {/* Canvas Area */}
        <div className="flex-1 flex items-center justify-center p-6 overflow-auto">
          <div
            style={{ transform: `scale(${zoom})`, transformOrigin: 'center' }}
            className="transition-transform duration-75 shadow-2xl rounded"
          >
            <canvas ref={canvasRef} className="max-h-[75vh] max-w-[70vw] object-contain rounded" />
          </div>
        </div>
      </div>

      {/* Right Properties Panel */}
      <div className="w-80 border-l border-zinc-800 bg-zinc-900 flex flex-col">
        {/* Tab Selector */}
        <div className="flex border-b border-zinc-800">
          <button
            onClick={() => setActiveTab('adjust')}
            className={`flex-1 py-3 text-xs font-semibold flex items-center justify-center gap-1.5 ${
              activeTab === 'adjust' ? 'border-b-2 border-indigo-500 text-white bg-zinc-800/40' : 'text-zinc-400 hover:text-zinc-200'
            }`}
          >
            <Sliders className="w-3.5 h-3.5" /> Adjust
          </button>
          <button
            onClick={() => setActiveTab('crop')}
            className={`flex-1 py-3 text-xs font-semibold flex items-center justify-center gap-1.5 ${
              activeTab === 'crop' ? 'border-b-2 border-indigo-500 text-white bg-zinc-800/40' : 'text-zinc-400 hover:text-zinc-200'
            }`}
          >
            <Crop className="w-3.5 h-3.5" /> Crop
          </button>
          <button
            onClick={() => setActiveTab('text')}
            className={`flex-1 py-3 text-xs font-semibold flex items-center justify-center gap-1.5 ${
              activeTab === 'text' ? 'border-b-2 border-indigo-500 text-white bg-zinc-800/40' : 'text-zinc-400 hover:text-zinc-200'
            }`}
          >
            <Type className="w-3.5 h-3.5" /> Text
          </button>
        </div>

        {/* Tab Contents */}
        <div className="flex-1 overflow-y-auto p-4 space-y-5">
          {activeTab === 'adjust' && (
            <div className="space-y-4">
              <div className="flex items-center justify-between">
                <span className="text-xs font-semibold text-zinc-400 uppercase tracking-wider">Lighting & Color</span>
                <button
                  onClick={handleResetAdjustments}
                  className="text-xs text-indigo-400 hover:text-indigo-300 flex items-center gap-1"
                >
                  <RotateCcw className="w-3 h-3" /> Reset
                </button>
              </div>

              {/* Exposure */}
              <div className="space-y-1.5">
                <div className="flex justify-between text-xs">
                  <span className="text-zinc-300">Exposure</span>
                  <span className="text-zinc-400 font-mono">{adjustments.exposure}</span>
                </div>
                <input
                  type="range"
                  min="-100"
                  max="100"
                  value={adjustments.exposure}
                  onChange={(e) => setAdjustments({ ...adjustments, exposure: Number(e.target.value) })}
                  className="w-full h-1.5 bg-zinc-700 rounded-lg appearance-none cursor-pointer accent-indigo-500"
                />
              </div>

              {/* Brightness */}
              <div className="space-y-1.5">
                <div className="flex justify-between text-xs">
                  <span className="text-zinc-300">Brightness</span>
                  <span className="text-zinc-400 font-mono">{adjustments.brightness}</span>
                </div>
                <input
                  type="range"
                  min="-100"
                  max="100"
                  value={adjustments.brightness}
                  onChange={(e) => setAdjustments({ ...adjustments, brightness: Number(e.target.value) })}
                  className="w-full h-1.5 bg-zinc-700 rounded-lg appearance-none cursor-pointer accent-indigo-500"
                />
              </div>

              {/* Contrast */}
              <div className="space-y-1.5">
                <div className="flex justify-between text-xs">
                  <span className="text-zinc-300">Contrast</span>
                  <span className="text-zinc-400 font-mono">{adjustments.contrast}</span>
                </div>
                <input
                  type="range"
                  min="-100"
                  max="100"
                  value={adjustments.contrast}
                  onChange={(e) => setAdjustments({ ...adjustments, contrast: Number(e.target.value) })}
                  className="w-full h-1.5 bg-zinc-700 rounded-lg appearance-none cursor-pointer accent-indigo-500"
                />
              </div>

              {/* Saturation */}
              <div className="space-y-1.5">
                <div className="flex justify-between text-xs">
                  <span className="text-zinc-300">Saturation</span>
                  <span className="text-zinc-400 font-mono">{adjustments.saturation}</span>
                </div>
                <input
                  type="range"
                  min="-100"
                  max="100"
                  value={adjustments.saturation}
                  onChange={(e) => setAdjustments({ ...adjustments, saturation: Number(e.target.value) })}
                  className="w-full h-1.5 bg-zinc-700 rounded-lg appearance-none cursor-pointer accent-indigo-500"
                />
              </div>

              {/* Temperature */}
              <div className="space-y-1.5">
                <div className="flex justify-between text-xs">
                  <span className="text-zinc-300">Temperature</span>
                  <span className="text-zinc-400 font-mono">{adjustments.temperature}</span>
                </div>
                <input
                  type="range"
                  min="-100"
                  max="100"
                  value={adjustments.temperature}
                  onChange={(e) => setAdjustments({ ...adjustments, temperature: Number(e.target.value) })}
                  className="w-full h-1.5 bg-zinc-700 rounded-lg appearance-none cursor-pointer accent-indigo-500"
                />
              </div>

              {/* Vignette */}
              <div className="space-y-1.5">
                <div className="flex justify-between text-xs">
                  <span className="text-zinc-300">Vignette</span>
                  <span className="text-zinc-400 font-mono">{adjustments.vignette}</span>
                </div>
                <input
                  type="range"
                  min="0"
                  max="100"
                  value={adjustments.vignette}
                  onChange={(e) => setAdjustments({ ...adjustments, vignette: Number(e.target.value) })}
                  className="w-full h-1.5 bg-zinc-700 rounded-lg appearance-none cursor-pointer accent-indigo-500"
                />
              </div>
            </div>
          )}

          {activeTab === 'crop' && (
            <div className="space-y-4">
              <span className="text-xs font-semibold text-zinc-400 uppercase tracking-wider">Aspect Ratio</span>
              <div className="grid grid-cols-2 gap-2">
                {['free', '1:1', '4:5', '16:9', '9:16'].map((ratio) => (
                  <button
                    key={ratio}
                    onClick={() => setCropAspect(ratio)}
                    className={`py-2 px-3 rounded text-xs font-medium border ${
                      cropAspect === ratio
                        ? 'border-indigo-500 bg-indigo-500/10 text-indigo-300'
                        : 'border-zinc-800 bg-zinc-800/50 text-zinc-400 hover:text-zinc-200'
                    }`}
                  >
                    {ratio === 'free' ? 'Freeform' : ratio}
                  </button>
                ))}
              </div>
              <p className="text-xs text-zinc-500 mt-2">
                Click and drag on the image to crop. Preset ratios constrain the crop window.
              </p>
            </div>
          )}

          {activeTab === 'text' && (
            <div className="space-y-4">
              <span className="text-xs font-semibold text-zinc-400 uppercase tracking-wider">Watermark / Overlay</span>
              <div className="space-y-2">
                <label className="text-xs text-zinc-300">Text Content</label>
                <input
                  type="text"
                  placeholder="e.g. © 2026 Gold Tone"
                  value={watermark}
                  onChange={(e) => setWatermark(e.target.value)}
                  className="w-full px-3 py-2 bg-zinc-800 border border-zinc-700 rounded text-xs text-zinc-100 placeholder-zinc-500 focus:outline-none focus:border-indigo-500"
                />
              </div>
              <p className="text-xs text-zinc-500">
                Watermark automatically renders on bottom-right with drop shadow.
              </p>
            </div>
          )}
        </div>

        {/* Export Footer */}
        <div className="p-4 border-t border-zinc-800 bg-zinc-950 space-y-2">
          <div className="text-xs font-semibold text-zinc-400 mb-2">Export Image</div>
          <div className="grid grid-cols-3 gap-2">
            <button
              onClick={() => handleExport('png')}
              className="py-2 px-3 rounded bg-zinc-800 hover:bg-zinc-700 text-xs font-medium text-white flex items-center justify-center gap-1.5 transition"
            >
              <Download className="w-3.5 h-3.5" /> PNG
            </button>
            <button
              onClick={() => handleExport('jpeg')}
              className="py-2 px-3 rounded bg-zinc-800 hover:bg-zinc-700 text-xs font-medium text-white flex items-center justify-center gap-1.5 transition"
            >
              <Download className="w-3.5 h-3.5" /> JPG
            </button>
            <button
              onClick={() => handleExport('webp')}
              className="py-2 px-3 rounded bg-indigo-600 hover:bg-indigo-500 text-xs font-medium text-white flex items-center justify-center gap-1.5 transition"
            >
              <Download className="w-3.5 h-3.5" /> WebP
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
