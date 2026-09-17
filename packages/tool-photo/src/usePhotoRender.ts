import { useEffect, useMemo, useRef, useState } from 'react';
import type { AssetItem, RawModuleId, RawStack } from '@duckfoot/core';
import {
  computeHistogram,
  decodeCameraRawNative,
  decodeRasterAsScene,
  renderRawStack,
  sceneToImageData,
} from '@duckfoot/media';
import type { Histogram, SceneBuffer } from '@duckfoot/media';

export interface PhotoRender {
  /** The developed preview, ready to draw. */
  image: ImageData | null;
  /** The source, undeveloped — the BEFORE half of the split view. */
  original: ImageData | null;
  histogram: Histogram | null;
  /** Enabled modules that had no implementation and were skipped. */
  stubbedModules: RawModuleId[];
  clippedHighPercent: number;
  sourceWidth: number;
  sourceHeight: number;
  loading: boolean;
  error: string | null;
}

/**
 * Decode once, re-render on every stack change.
 *
 * DESIGN.md invariant 4: preview works on decimated data and must stay interactive;
 * full resolution happens at export only. `decodeRasterAsScene` caps the long edge
 * at 1600px, so a slider drag re-runs the pipeline over ~1.7M pixels rather than
 * 24M. Invariant 2 is satisfied by there being exactly one `renderRawStack` — export
 * will call the same function with `quality: 'export'` and an undecimated scene.
 */
export function usePhotoRender(asset: AssetItem | null, stack: RawStack): PhotoRender {
  const [scene, setScene] = useState<SceneBuffer | null>(null);
  const [original, setOriginal] = useState<ImageData | null>(null);
  const [dims, setDims] = useState<{ width: number; height: number }>({ width: 0, height: 0 });
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const tokenRef = useRef(0);

  useEffect(() => {
    if (!asset || asset.type !== 'photo') {
      setScene(null);
      setOriginal(null);
      setError(null);
      return;
    }

    const token = ++tokenRef.current;
    setLoading(true);
    setError(null);

    const isRawFile = /\.(cr[23]|nef|arw|dng|raw|orf|rw2)$/i.test(asset.name || asset.url || '');
    const isLocalPath = asset.url && !asset.url.startsWith('data:') && !asset.url.startsWith('blob:') && !asset.url.startsWith('http');

    if (isRawFile && isLocalPath) {
      decodeCameraRawNative(asset.url!)
        .then((res) => {
          if (tokenRef.current !== token) return;
          setScene(res.scene);
          setOriginal(sceneToImageData(res.scene));
          setDims({ width: res.metadata.width, height: res.metadata.height });
          setLoading(false);
        })
        .catch((err) => {
          if (tokenRef.current !== token) return;
          setError(`${asset.name}: ${(err as Error).message || 'could not decode RAW'}`);
          setScene(null);
          setOriginal(null);
          setLoading(false);
        });
      return () => {
        tokenRef.current++;
      };
    }

    const image = new Image();
    image.crossOrigin = 'anonymous';
    image.onload = () => {
      if (tokenRef.current !== token) return;
      try {
        const decoded = decodeRasterAsScene(image);
        setScene(decoded);
        setOriginal(sceneToImageData(decoded));
        setDims({ width: image.naturalWidth, height: image.naturalHeight });

        if (typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window) {
          import('@tauri-apps/api/core')
            .then(({ invoke }) => {
              invoke('set_photo_scene', {
                pixels: Array.from(decoded.data),
                width: decoded.width,
                height: decoded.height,
              }).catch(() => {});
            })
            .catch(() => {});
        }
      } catch (cause) {
        // DESIGN.md: "Failures surface in the UI, never only in the console. A file
        // that won't decode says so, in place, with its filename."
        setError(`${asset.name} could not be decoded`);
        setScene(null);
        setOriginal(null);
      } finally {
        setLoading(false);
      }
    };
    image.onerror = () => {
      if (tokenRef.current !== token) return;
      setError(`${asset.name} could not be loaded`);
      setScene(null);
      setOriginal(null);
      setLoading(false);
    };

    if (!asset.url) {
      image.src = createSamplePhotoScene(asset.name);
    } else {
      image.src = asset.url;
    }

    return () => {
      tokenRef.current++;
    };
  }, [asset]);

  return useMemo(() => {
    if (!scene) {
      return {
        image: null,
        original,
        histogram: null,
        stubbedModules: [],
        clippedHighPercent: 0,
        sourceWidth: dims.width,
        sourceHeight: dims.height,
        loading,
        error,
      };
    }

    const { scene: developed, stubbedModules } = renderRawStack(scene, stack, { quality: 'preview' });
    const histogram = computeHistogram(developed);
    return {
      image: sceneToImageData(developed),
      original,
      histogram,
      stubbedModules,
      clippedHighPercent: histogram.clippedHighFraction * 100,
      sourceWidth: dims.width,
      sourceHeight: dims.height,
      loading,
      error,
    };
  }, [scene, stack, original, dims, loading, error]);
}

/**
 * Generates an in-memory photographic instrument scene with rich dynamic range,
 * gradients, and hardware details for fixtures without external URLs.
 */
function createSamplePhotoScene(name: string, width = 1200, height = 800): string {
  if (typeof document === 'undefined') return '';
  const canvas = document.createElement('canvas');
  canvas.width = width;
  canvas.height = height;
  const ctx = canvas.getContext('2d');
  if (!ctx) return '';

  // Rich studio photographic backdrop with warm vignette
  const grad = ctx.createRadialGradient(
    width * 0.45,
    height * 0.45,
    60,
    width * 0.5,
    height * 0.5,
    width * 0.65
  );
  grad.addColorStop(0, '#f2d8a7');
  grad.addColorStop(0.25, '#bc823e');
  grad.addColorStop(0.55, '#6a3614');
  grad.addColorStop(0.85, '#261005');
  grad.addColorStop(1, '#080301');
  ctx.fillStyle = grad;
  ctx.fillRect(0, 0, width, height);

  // Wooden instrument headstock
  ctx.save();
  ctx.translate(width * 0.5, height * 0.5);

  // Soft drop shadow
  ctx.shadowColor = 'rgba(0,0,0,0.8)';
  ctx.shadowBlur = 40;
  ctx.shadowOffsetY = 15;

  // Headstock silhouette
  ctx.fillStyle = '#1c0e06';
  ctx.strokeStyle = '#c9992e';
  ctx.lineWidth = 3.5;
  ctx.beginPath();
  ctx.roundRect(-150, -230, 300, 460, [45, 45, 12, 12]);
  ctx.fill();
  ctx.shadowColor = 'transparent';
  ctx.stroke();

  // Fine brass inlay and mother-of-pearl brand logo
  ctx.fillStyle = '#f7eedd';
  ctx.font = 'italic bold 42px serif';
  ctx.textAlign = 'center';
  ctx.fillText('Gold Tone', 0, -85);

  ctx.font = '13px monospace';
  ctx.fillStyle = '#c9992e';
  ctx.letterSpacing = '0.18em';
  ctx.fillText('VINTAGE CRAFT', 0, -42);

  // Tuning machine posts (brass and chrome hardware)
  [-95, 95].forEach((x) => {
    [-150, -50, 50, 150].forEach((y) => {
      ctx.beginPath();
      ctx.arc(x, y, 15, 0, Math.PI * 2);
      ctx.fillStyle = '#c9992e';
      ctx.fill();
      ctx.strokeStyle = '#7a5f22';
      ctx.lineWidth = 2;
      ctx.stroke();

      // Post center
      ctx.beginPath();
      ctx.arc(x, y, 6, 0, Math.PI * 2);
      ctx.fillStyle = '#3a2f14';
      ctx.fill();
    });
  });

  ctx.restore();

  // Photo identification tag in corner
  ctx.fillStyle = 'rgba(10, 10, 10, 0.75)';
  ctx.fillRect(24, height - 64, 300, 40);
  ctx.strokeStyle = 'rgba(201, 153, 46, 0.4)';
  ctx.strokeRect(24, height - 64, 300, 40);
  ctx.fillStyle = '#e5e5e5';
  ctx.font = '13px monospace';
  ctx.fillText(`${name} · 14-bit RAW`, 38, height - 39);

  return canvas.toDataURL('image/jpeg', 0.92);
}

