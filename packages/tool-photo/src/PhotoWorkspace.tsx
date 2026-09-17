import { useEffect, useRef, useState } from 'react';
import type { BarrelRegionProps } from '@duckfoot/ui';
import { ChipGroup, IconButton, MonoValue, ToggleChip, useDragGesture } from '@duckfoot/ui';
import { usePhoto } from './PhotoContext';

/**
 * The develop viewport: toolbar, the image with its before/after split, filmstrip.
 */
export function PhotoWorkspace(props: BarrelRegionProps) {
  const { render, view, asset } = usePhoto();
  const [zoom, setZoom] = useState(0.38);
  const [overlay, setOverlay] = useState<'clip' | 'focus' | 'gamut' | null>('clip');

  const afterRef = useRef<HTMLCanvasElement>(null);
  const beforeRef = useRef<HTMLCanvasElement>(null);
  const frameRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    paint(afterRef.current, render.image);
  }, [render.image]);
  useEffect(() => {
    paint(beforeRef.current, render.original);
  }, [render.original]);

  const startSplit = useDragGesture<{ start: number }>({
    onStart: () => ({ start: view.split }),
    onMove: (_state, event) => view.setSplit(splitFrom(frameRef.current, event)),
    onCommit: (_state, event) => view.setSplit(splitFrom(frameRef.current, event)),
    onCancel: (state) => view.setSplit(state.start),
  });

  const photoAssets = props.assets.filter((item) => item.type === 'photo');

  return (
    <div className="df-photo">
      <div className="df-photo__toolbar">
        <div className="df-photo__tool-group">
          <IconButton glyph="⟳" label="Rotate" />
          <IconButton glyph="⇄" label="Flip horizontal" />
        </div>
        <span className="df-divider" />
        <ChipGroup>
          {(['clip', 'focus', 'gamut'] as const).map((id) => (
            <ToggleChip
              key={id}
              label={id.toUpperCase()}
              active={overlay === id}
              onClick={() => setOverlay(overlay === id ? null : id)}
              title={id === 'clip' ? 'Highlight clipping' : 'Not implemented yet'}
              disabled={id !== 'clip'}
            />
          ))}
        </ChipGroup>
        <span className="df-divider" />
        <span className="df-mono df-mono--dim">soft proof — Display P3</span>
        <div className="df-photo__spacer" />
        <div className="df-photo__zoom">
          <IconButton glyph="−" label="Zoom out" onClick={() => setZoom((z) => Math.max(0.05, z - 0.05))} />
          <MonoValue>{`${Math.round(zoom * 100)}%`}</MonoValue>
          <IconButton glyph="+" label="Zoom in" onClick={() => setZoom((z) => Math.min(4, z + 0.05))} />
          <ToggleChip label="FIT" active={false} onClick={() => setZoom(0.38)} />
        </div>
      </div>

      <div className="df-photo__stage">
        {render.error ? (
          <div className="df-photo__error" role="alert">
            {render.error}
          </div>
        ) : !asset ? (
          <div className="df-photo__empty">Select an image from the Shelf to develop it.</div>
        ) : (
          <div className="df-photo__frame" ref={frameRef}>
            <canvas ref={afterRef} className="df-photo__canvas" />
            {/* BEFORE is the same pixels with an un-developed stack, clipped to the
                left of the divider. Same render function, different stack. */}
            <div className="df-photo__before" style={{ width: `${view.split * 100}%` }}>
              <canvas ref={beforeRef} className="df-photo__canvas" />
              <span className="df-photo__tag df-mono">BEFORE</span>
            </div>
            <div
              className="df-photo__divider"
              style={{ left: `${view.split * 100}%` }}
              onPointerDown={startSplit}
              role="separator"
              aria-label="Before / after split"
            >
              <span className="df-photo__grip" />
              <span className="df-photo__tag df-photo__tag--after df-mono">AFTER</span>
            </div>

            <div className="df-photo__readout df-mono">
              {render.histogram ? (
                <>
                  <span>{`clipped ${render.clippedHighPercent.toFixed(1)}%`}</span>
                  {render.stubbedModules.length > 0 ? (
                    <span className="df-photo__readout-warn">
                      {`${render.stubbedModules.length} module${
                        render.stubbedModules.length === 1 ? '' : 's'
                      } skipped`}
                    </span>
                  ) : null}
                </>
              ) : (
                <span>—</span>
              )}
            </div>
            <div className="df-photo__note df-mono">
              preview is approximate · export re-runs the pipeline at full resolution
            </div>
          </div>
        )}
      </div>

      <div className="df-filmstrip">
        <div className="df-filmstrip__head">
          <span className="df-panel-heading__label">FILMSTRIP</span>
          <span className="df-mono df-mono--dim">{`${photoAssets.length} image${
            photoAssets.length === 1 ? '' : 's'
          }`}</span>
        </div>
        <div className="df-filmstrip__strip">
          {photoAssets.map((item) => (
            <button
              key={item.id}
              type="button"
              className={`df-filmstrip__frame${
                item.id === props.activeAssetId ? ' df-filmstrip__frame--active' : ''
              }`}
              onClick={() => props.onSelectAsset(item.id, false)}
              title={item.name}
            >
              <span className="df-filmstrip__id df-mono">{shortName(item.name)}</span>
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}

function paint(canvas: HTMLCanvasElement | null, image: ImageData | null) {
  if (!canvas || !image) return;
  canvas.width = image.width;
  canvas.height = image.height;
  canvas.getContext('2d')?.putImageData(image, 0, 0);
}

function splitFrom(frame: HTMLElement | null, event: PointerEvent): number {
  if (!frame) return 0.5;
  const rect = frame.getBoundingClientRect();
  return Math.max(0, Math.min(1, (event.clientX - rect.left) / rect.width));
}

function shortName(name: string): string {
  const base = name.replace(/\.[^/.]+$/, '');
  return base.length > 8 ? base.slice(-8) : base;
}
