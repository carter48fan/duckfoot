export interface VideoExportOptions {
  width: number;
  height: number;
  fps: number;
  duration: number; // in seconds
  format: 'mp4' | 'webm';
  onProgress?: (progress: number) => void;
  renderFrame: (time: number, ctx: CanvasRenderingContext2D) => Promise<void>;
  audioBuffer?: AudioBuffer | null;
}

export class VideoExporter {
  private isCancelled = false;

  cancel() {
    this.isCancelled = true;
  }

  async export(options: VideoExportOptions): Promise<Blob> {
    const { width, height, fps, duration, format, onProgress, renderFrame, audioBuffer } = options;
    const canvas = document.createElement('canvas');
    canvas.width = width;
    canvas.height = height;
    const ctx = canvas.getContext('2d');
    if (!ctx) throw new Error('Failed to create canvas context for video export');

    const totalFrames = Math.max(1, Math.ceil(duration * fps));

    // Try WebCodecs with mediabunny if available
    try {
      const { Output, Mp4OutputFormat, WebMOutputFormat, BufferTarget, CanvasSource, AudioBufferSource } = await import('mediabunny');

      const outputFormat = format === 'webm' ? new WebMOutputFormat() : new Mp4OutputFormat();
      const output = new Output({
        format: outputFormat,
        target: new BufferTarget(),
      });

      const videoSource = new CanvasSource(canvas, {
        codec: format === 'webm' ? 'vp9' : 'avc',
        bitrate: 4_000_000,
      });

      output.addVideoTrack(videoSource, { frameRate: fps });

      let audioSource: any = null;
      if (audioBuffer) {
        audioSource = new AudioBufferSource({
          codec: format === 'webm' ? 'opus' : 'aac',
          bitrate: 192_000,
        });
        output.addAudioTrack(audioSource);
      }

      await output.start();

      if (audioSource && audioBuffer) {
        await audioSource.add(audioBuffer);
        audioSource.close();
      }

      for (let i = 0; i < totalFrames; i++) {
        if (this.isCancelled) {
          await output.cancel();
          throw new Error('Export cancelled by user');
        }

        const time = i / fps;
        await renderFrame(time, ctx);
        await videoSource.add(time, 1 / fps);

        if (onProgress) onProgress((i + 1) / totalFrames);
      }

      videoSource.close();
      await output.finalize();

      const buffer = output.target.buffer;
      if (!buffer) throw new Error('Export produced empty buffer');
      return new Blob([buffer], { type: format === 'webm' ? 'video/webm' : 'video/mp4' });
    } catch (e: any) {
      if (this.isCancelled) throw e;
      console.warn('WebCodecs export failed or unavailable, falling back to MediaRecorder:', e);
      return this.exportWithMediaRecorder(options, canvas, ctx);
    }
  }

  private async exportWithMediaRecorder(
    options: VideoExportOptions,
    canvas: HTMLCanvasElement,
    ctx: CanvasRenderingContext2D
  ): Promise<Blob> {
    const { fps, duration, onProgress, renderFrame } = options;
    const stream = canvas.captureStream(fps);
    const mimeType = MediaRecorder.isTypeSupported('video/webm;codecs=vp9')
      ? 'video/webm;codecs=vp9'
      : 'video/webm';

    const recorder = new MediaRecorder(stream, { mimeType });
    const chunks: Blob[] = [];

    recorder.ondataavailable = (e) => {
      if (e.data && e.data.size > 0) chunks.push(e.data);
    };

    const completionPromise = new Promise<Blob>((resolve, reject) => {
      recorder.onstop = () => resolve(new Blob(chunks, { type: 'video/webm' }));
      recorder.onerror = (e) => reject(e);
    });

    recorder.start();

    const totalFrames = Math.max(1, Math.ceil(duration * fps));
    for (let i = 0; i < totalFrames; i++) {
      if (this.isCancelled) {
        recorder.stop();
        throw new Error('Export cancelled');
      }

      const time = i / fps;
      await renderFrame(time, ctx);
      if (onProgress) onProgress((i + 1) / totalFrames);
      await new Promise((r) => setTimeout(r, 1000 / fps));
    }

    recorder.stop();
    return completionPromise;
  }
}
