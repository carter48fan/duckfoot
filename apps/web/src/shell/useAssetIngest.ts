import { useCallback } from 'react';
import { nanoid } from 'nanoid';
import type { AssetItem, Command, MediaType } from '@duckfoot/core';

function mediaTypeOf(file: File): MediaType {
  if (file.type.startsWith('audio/')) return 'audio';
  if (file.type.startsWith('video/')) return 'video';
  return 'photo';
}

/** Reads intrinsic dimensions / duration without blocking the import on failure. */
function probe(item: AssetItem): Promise<AssetItem> {
  return new Promise((resolve) => {
    if (item.type === 'photo') {
      const img = new Image();
      img.onload = () => resolve({ ...item, width: img.naturalWidth, height: img.naturalHeight });
      img.onerror = () => resolve(item);
      img.src = item.url;
      return;
    }
    const el = document.createElement(item.type === 'video' ? 'video' : 'audio');
    el.onloadedmetadata = () => {
      const next: AssetItem = { ...item, duration: el.duration };
      if (item.type === 'video') {
        const video = el as HTMLVideoElement;
        next.width = video.videoWidth;
        next.height = video.videoHeight;
      }
      resolve(next);
    };
    el.onerror = () => resolve(item);
    el.src = item.url;
  });
}

/**
 * File import for the Shelf.
 *
 * The logic is lifted from the demolished CreativeSuite, with its race fixed: that
 * version called setState from inside each async metadata callback while reading a
 * stale `activeAssetId` closure, so importing several files at once set the active
 * asset repeatedly and appended assets in a nondeterministic order. Here every file
 * is probed first and the whole batch lands as one command — which also means one
 * undo entry rather than one per file.
 */
export function useAssetIngest(dispatch: (command: Command) => void) {
  return useCallback(
    async (files: FileList | File[] | null) => {
      if (!files) return;
      const list = Array.from(files);
      if (list.length === 0) return;

      const probed = await Promise.all(
        list.map((file) =>
          probe({
            id: nanoid(),
            name: file.name,
            type: mediaTypeOf(file),
            url: URL.createObjectURL(file),
            file,
            size: file.size,
            createdAt: Date.now(),
          })
        )
      );

      dispatch({ type: 'suite/addAssets', assets: probed });
    },
    [dispatch]
  );
}
