import type { BarrelDefinition } from '@duckfoot/ui';
import { PhotoProvider } from './PhotoContext';
import { PhotoWorkspace } from './PhotoWorkspace';
import { PhotoInspector } from './PhotoInspector';
import { PhotoTransport, PhotoStatus, PhotoShelfPanels } from './PhotoRegions';

export const photoBarrel: BarrelDefinition = {
  id: 'photo',
  label: 'Photo',
  Provider: PhotoProvider,
  Workspace: PhotoWorkspace,
  Inspector: PhotoInspector,
  Transport: PhotoTransport,
  ShelfPanels: PhotoShelfPanels,
  Status: PhotoStatus,
};

export { PhotoProvider, PhotoWorkspace, PhotoInspector, PhotoTransport, PhotoStatus, PhotoShelfPanels };
export * from './PhotoContext';
export * from './usePhotoRender';
