import type { AssetItem, DuckfootDocument } from '@duckfoot/core';
import { createRawStack } from '@duckfoot/core';

export const FIXTURE_PHOTO_ASSETS: AssetItem[] = [
  {
    id: 'asset-4412',
    name: 'IMG_4412.CR3',
    type: 'photo',
    url: '',
    size: 32_400_000,
    width: 6000,
    height: 4000,
    createdAt: Date.now() - 3600000 * 2,
  },
  {
    id: 'asset-headstock',
    name: 'headstock-final.tif',
    type: 'photo',
    url: '',
    size: 137_000_000,
    width: 2520,
    height: 3150,
    createdAt: Date.now() - 3600000,
  },
  {
    id: 'asset-4413',
    name: 'IMG_4413.CR3',
    type: 'photo',
    url: '',
    size: 31_800_000,
    width: 6000,
    height: 4000,
    createdAt: Date.now() - 3600000 * 1.8,
  },
  {
    id: 'asset-4414',
    name: 'IMG_4414.CR3',
    type: 'photo',
    url: '',
    size: 33_100_000,
    width: 6000,
    height: 4000,
    createdAt: Date.now() - 3600000 * 1.6,
  },
  {
    id: 'asset-4415',
    name: 'IMG_4415.CR3',
    type: 'photo',
    url: '',
    size: 32_900_000,
    width: 6000,
    height: 4000,
    createdAt: Date.now() - 3600000 * 1.4,
  },
  {
    id: 'asset-4416',
    name: 'IMG_4416.CR3',
    type: 'photo',
    url: '',
    size: 32_200_000,
    width: 6000,
    height: 4000,
    createdAt: Date.now() - 3600000 * 1.2,
  },
  {
    id: 'asset-4417',
    name: 'IMG_4417.CR3',
    type: 'photo',
    url: '',
    size: 32_700_000,
    width: 6000,
    height: 4000,
    createdAt: Date.now() - 3600000,
  },
];

export const FIXTURE_AUDIO_ASSETS: AssetItem[] = [
  {
    id: 'asset-drums',
    name: 'stem-drums.wav',
    type: 'audio',
    url: '',
    size: 39_168_000,
    duration: 204.0,
    createdAt: Date.now() - 3600000 * 3,
  },
  {
    id: 'asset-bass',
    name: 'stem-bass.wav',
    type: 'audio',
    url: '',
    size: 39_168_000,
    duration: 204.0,
    createdAt: Date.now() - 3600000 * 3,
  },
  {
    id: 'asset-vox',
    name: 'stem-vox.wav',
    type: 'audio',
    url: '',
    size: 39_168_000,
    duration: 204.0,
    createdAt: Date.now() - 3600000 * 3,
  },
  {
    id: 'asset-mix',
    name: 'mix-headstock.wav',
    type: 'audio',
    url: '',
    size: 39_168_000,
    duration: 204.0,
    createdAt: Date.now() - 3600000,
  },
];

export function createFixtureDocument(): DuckfootDocument {
  const photoStack = createRawStack();

  return {
    suite: {
      assets: [...FIXTURE_PHOTO_ASSETS, ...FIXTURE_AUDIO_ASSETS],
    },
    photo: {
      assetId: 'asset-4412',
      kind: 'raw',
      stack: photoStack,
      adjustments: {
        exposure: 0,
        contrast: 0,
        saturation: 0,
        brightness: 0,
        temperature: 0,
        vignette: 0,
      },
      watermark: '',
    },
    audio: {
      laneOrder: ['lane-drums', 'lane-bass', 'lane-vox'],
      lanes: {
        'lane-drums': {
          id: 'lane-drums',
          name: 'drums',
          assetId: 'asset-drums',
          gainDb: 0,
          muted: false,
          soloed: false,
          isPhaseReference: true,
          phase: {
            polarityInverted: false,
            offsetSeconds: 0,
            rotationDegrees: 0,
          },
        },
        'lane-bass': {
          id: 'lane-bass',
          name: 'bass',
          assetId: 'asset-bass',
          gainDb: 0,
          muted: false,
          soloed: false,
          isPhaseReference: false,
          phase: {
            polarityInverted: false,
            offsetSeconds: 0,
            rotationDegrees: 0,
          },
        },
        'lane-vox': {
          id: 'lane-vox',
          name: 'vox',
          assetId: 'asset-vox',
          gainDb: -1.5,
          muted: false,
          soloed: false,
          isPhaseReference: false,
          phase: {
            polarityInverted: false,
            offsetSeconds: -0.00142,
            rotationDegrees: 38,
          },
        },
      },
      selection: {
        inSeconds: 42.118,
        outSeconds: 78.226,
      },
      durationSeconds: 204.0,
      sampleRate: 48000,
      edits: [],
      envelope: {
        interpolation: 'lin',
        nodes: [
          { timeSeconds: 42.118, gainDb: 0.0 },
          { timeSeconds: 48.940, gainDb: -7.5 },
          { timeSeconds: 64.300, gainDb: -7.5 },
          { timeSeconds: 67.600, gainDb: 0.0 },
        ],
      },
    },
  };
}
