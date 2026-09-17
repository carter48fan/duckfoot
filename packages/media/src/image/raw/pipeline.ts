import { RAW_MODULE_META, RAW_MODULE_ORDER } from '@duckfoot/core';
import type { RawModuleId, RawModuleInstance, RawStack } from '@duckfoot/core';
import type { SceneBuffer } from './scene';
import { RAW_MODULE_APPLY } from './modules';

/**
 * DESIGN.md invariant 2: "One render function serves preview and export. If you
 * can't export it, don't render it. Because a second export path always drifts from
 * what the user was looking at."
 *
 * There is therefore exactly one of these. Preview passes a decimated scene;
 * export passes a full-resolution one from inside a worker. `quality` may only let
 * a module choose a cheaper kernel for the same operation — it must never change
 * which modules run. If it ever does, invariant 2 is broken and the break will be
 * invisible until someone compares an export to the screen.
 */
export interface RenderRawOptions {
  quality: 'preview' | 'export';
  signal?: AbortSignal;
  onProgress?: (moduleId: RawModuleId, index: number, total: number) => void;
}

export interface RenderRawResult {
  scene: SceneBuffer;
  /**
   * Modules that were enabled but had no implementation, so did not run.
   *
   * This is the one sanctioned form of stub in a composed pipeline: skipping and
   * reporting. Throwing would take down thirteen working modules for one missing
   * one; passing through silently would let the screen claim an effect that never
   * happened. The Inspector badges every id in this list.
   */
  stubbedModules: RawModuleId[];
}

export function renderRawStack(
  input: SceneBuffer,
  stack: RawStack,
  options: RenderRawOptions
): RenderRawResult {
  const stubbed: RawModuleId[] = [];
  let scene: SceneBuffer = {
    data: new Float32Array(input.data),
    width: input.width,
    height: input.height,
    colorspace: input.colorspace,
    scale: input.scale,
  };

  const enabled = RAW_MODULE_ORDER.filter((id) => stack[id].enabled);

  enabled.forEach((id, index) => {
    if (options.signal?.aborted) return;

    const meta = RAW_MODULE_META[id];
    if (meta.status === 'stub') {
      stubbed.push(id);
      return;
    }

    const instance = stack[id] as RawModuleInstance;
    const apply = RAW_MODULE_APPLY[id] as (
      s: SceneBuffer,
      p: typeof instance.params,
      inPlace?: boolean
    ) => SceneBuffer;
    scene = apply(scene, instance.params, true);
    options.onProgress?.(id, index, enabled.length);
  });

  return { scene, stubbedModules: stubbed };
}
