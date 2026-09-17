import type { BarrelDefinition } from '@duckfoot/ui';
import { photoBarrel } from '@duckfoot/tool-photo';
import { audioBarrel } from '@duckfoot/tool-audio';
import { makeBarrelStub } from './BarrelStub';

const videoStub = makeBarrelStub(
  'Video Editor',
  'Three fixed tracks (overlay, main, audio). Reference implementation preserved; awaiting turn 3 rebuild.'
);

export const videoBarrel: BarrelDefinition = {
  id: 'video',
  label: 'Video',
  Workspace: videoStub.Workspace,
  Inspector: videoStub.Inspector,
  Transport: videoStub.Transport,
  ShelfPanels: videoStub.ShelfPanels,
  Status: videoStub.Status,
};

export const BARRELS: readonly BarrelDefinition[] = [photoBarrel, audioBarrel, videoBarrel];
