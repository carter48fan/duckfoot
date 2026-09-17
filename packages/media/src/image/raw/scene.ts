/**
 * Scene-referred linear RGB.
 *
 * The RAW pipeline works in linear float, not 8-bit gamma-encoded sRGB. That is the
 * whole reason it cannot share code with `../filters.ts`: the display-referred path
 * clips and posterises by design, which is fine for a JPEG and wrong for a 14-bit
 * capture with four stops of headroom above white.
 *
 * `scale` carries DESIGN.md invariant 4 through the pipeline: preview renders at a
 * fraction of full resolution and must stay interactive; export renders at 1.0 off
 * the main thread. Both go through the same function.
 */
export interface SceneBuffer {
  /** Interleaved RGB, three floats per pixel. Linear light, nominally 0..1 but
   *  values above 1 are legal and meaningful — that is the headroom. */
  readonly data: Float32Array;
  readonly width: number;
  readonly height: number;
  readonly colorspace: 'camera-rgb' | 'linear-rec2020' | 'display';
  /** 1 = full resolution. */
  readonly scale: number;
}

export function createScene(
  width: number,
  height: number,
  colorspace: SceneBuffer['colorspace'] = 'linear-rec2020',
  scale = 1
): SceneBuffer {
  return { data: new Float32Array(width * height * 3), width, height, colorspace, scale };
}

/**
 * Allocates an output the same shape as the input.
 *
 * Every module allocates rather than writing through its input. At preview scale a
 * 1600×1067 buffer is about 20 MB, so one allocation per enabled module is cheap;
 * at export scale it is not, which is why export belongs in a worker and why a
 * scratch-buffer pool is the obvious later optimisation. Correctness first.
 */
export function likeScene(scene: SceneBuffer, colorspace?: SceneBuffer['colorspace']): SceneBuffer {
  return {
    data: new Float32Array(scene.data.length),
    width: scene.width,
    height: scene.height,
    colorspace: colorspace ?? scene.colorspace,
    scale: scene.scale,
  };
}

/** Per-pixel map. The common shape for a pointwise module. */
export function mapPixels(
  scene: SceneBuffer,
  fn: (r: number, g: number, b: number) => [number, number, number],
  colorspace?: SceneBuffer['colorspace']
): SceneBuffer {
  const out = likeScene(scene, colorspace);
  const src = scene.data;
  const dst = out.data;
  for (let i = 0; i < src.length; i += 3) {
    const [r, g, b] = fn(src[i], src[i + 1], src[i + 2]);
    dst[i] = r;
    dst[i + 1] = g;
    dst[i + 2] = b;
  }
  return out;
}
