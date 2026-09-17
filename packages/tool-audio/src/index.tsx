import type { BarrelDefinition } from '@duckfoot/ui';
import { AudioProvider } from './AudioContext';
import { AudioWorkspace } from './AudioWorkspace';
import { AudioInspector } from './AudioInspector';
import { AudioTransport, AudioStatus, AudioShelfPanels } from './AudioRegions';

export const audioBarrel: BarrelDefinition = {
  id: 'audio',
  label: 'Audio',
  Provider: AudioProvider,
  Workspace: AudioWorkspace,
  Inspector: AudioInspector,
  Transport: AudioTransport,
  ShelfPanels: AudioShelfPanels,
  Status: AudioStatus,
};

export {
  AudioProvider,
  AudioWorkspace,
  AudioInspector,
  AudioTransport,
  AudioStatus,
  AudioShelfPanels,
};
export * from './AudioContext';
