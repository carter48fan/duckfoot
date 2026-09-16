export type TrackType = 'overlay' | 'main' | 'audio';

export interface TimelineClip {
  id: string;
  track: TrackType;
  name: string;
  assetId?: string;
  url?: string;
  startTime: number;     // position on timeline (seconds)
  duration: number;      // duration on timeline (seconds)
  trimStart: number;     // offset into source media (seconds)
  volume?: number;       // for video/audio clips
  textContent?: string;  // for text overlay clips
  fontSize?: number;
  textColor?: string;
}

export interface VideoProject {
  id: string;
  title: string;
  width: number;
  height: number;
  fps: number;
  clips: TimelineClip[];
}
