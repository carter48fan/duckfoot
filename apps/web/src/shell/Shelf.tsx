import { useRef } from 'react';
import type { AssetItem } from '@duckfoot/core';
import { formatFileSize } from '@duckfoot/core';

const KIND_LABEL: Record<AssetItem['type'], string> = {
  photo: 'IMG',
  audio: 'WAV',
  video: 'MP4',
};

function metaLine(asset: AssetItem): string {
  const parts: string[] = [];
  if (asset.width && asset.height) parts.push(`${asset.width} × ${asset.height}`);
  if (asset.duration) {
    const mins = Math.floor(asset.duration / 60);
    const secs = Math.floor(asset.duration % 60);
    parts.push(`${mins}:${secs.toString().padStart(2, '0')}`);
  }
  parts.push(formatFileSize(asset.size));
  return parts.join(' · ');
}

/**
 * The Shelf — DESIGN.md calls it the stock: "One asset list, shared by all three,
 * and the only route between them."
 *
 * The list is genuinely shared. What is not shared is the panel below it, which each
 * barrel fills for itself — 2a hangs SNAPSHOTS and HISTORY there, 2b hangs
 * CORRELATION. Hence the `panels` slot.
 *
 * Multi-select is new furniture that the old shelf never had, and 2a needs it:
 * "Paste to 5 selected" is not expressible against a single active asset.
 */
export function Shelf({
  assets,
  selectedIds,
  activeId,
  onSelect,
  onImport,
  panels,
  label,
}: {
  assets: AssetItem[];
  selectedIds: string[];
  activeId: string | null;
  onSelect: (id: string, additive: boolean) => void;
  onImport: (files: FileList | File[] | null) => void;
  panels: React.ReactNode;
  label: string;
}) {
  const inputRef = useRef<HTMLInputElement>(null);

  return (
    <aside className="df-shelf">
      <div className="df-shelf__header">
        <span className="df-shelf__title">SHELF</span>
        <span className="df-shelf__hint df-mono df-mono--dim">{label}</span>
        <button
          type="button"
          className="df-shelf__add"
          onClick={() => inputRef.current?.click()}
          title="Import media"
          aria-label="Import media"
        >
          +
        </button>
        <input
          ref={inputRef}
          type="file"
          multiple
          accept="image/*,audio/*,video/*"
          hidden
          onChange={(event) => {
            onImport(event.target.files);
            // Reset so re-picking the same file fires change again.
            event.target.value = '';
          }}
        />
      </div>

      <div
        className="df-shelf__list"
        onDragOver={(event) => event.preventDefault()}
        onDrop={(event) => {
          event.preventDefault();
          onImport(event.dataTransfer.files);
        }}
      >
        {assets.length === 0 ? (
          <button type="button" className="df-shelf__empty" onClick={() => inputRef.current?.click()}>
            Drop media here, or click to import
          </button>
        ) : (
          assets.map((asset) => {
            const selected = selectedIds.includes(asset.id);
            return (
              <button
                key={asset.id}
                type="button"
                className={`df-shelf__row${selected ? ' df-shelf__row--selected' : ''}${
                  asset.id === activeId ? ' df-shelf__row--active' : ''
                }`}
                onClick={(event) => onSelect(asset.id, event.shiftKey || event.metaKey || event.ctrlKey)}
              >
                <span className="df-shelf__kind df-mono">{KIND_LABEL[asset.type]}</span>
                <span className="df-shelf__text">
                  <span className="df-shelf__name">{asset.name}</span>
                  <span className="df-shelf__meta df-mono">{metaLine(asset)}</span>
                </span>
              </button>
            );
          })
        )}
      </div>

      <div className="df-shelf__panels">{panels}</div>

      <div className="df-shelf__footer df-mono">
        {assets.length} {assets.length === 1 ? 'asset' : 'assets'} · exports land here
      </div>
    </aside>
  );
}
