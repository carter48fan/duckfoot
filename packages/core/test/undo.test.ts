import { describe, expect, it } from 'vitest';
import {
  createAppState,
  currentDocument,
  execute,
  undo,
  redo,
  jumpTo,
  canUndo,
  canRedo,
  visibleHistory,
} from '../src/document/undo';

describe('Document Undo & Snapshot Store', () => {
  it('initializes with unedited original state', () => {
    const state = createAppState();
    expect(state.index).toBe(0);
    expect(state.history).toHaveLength(1);
    expect(state.history[0].label).toBe('original');
    expect(canUndo(state)).toBe(false);
    expect(canRedo(state)).toBe(false);
  });

  it('pushes new snapshots on commands and supports undo/redo', () => {
    let state = createAppState();
    state = execute(state, {
      type: 'photo/setModuleParam',
      moduleId: 'exposure',
      patch: { ev: 1.5 },
    });

    expect(state.index).toBe(1);
    expect(canUndo(state)).toBe(true);
    expect(canRedo(state)).toBe(false);
    expect(currentDocument(state).photo.stack.exposure.params.ev).toBe(1.5);

    // Undo
    state = undo(state);
    expect(state.index).toBe(0);
    expect(canUndo(state)).toBe(false);
    expect(canRedo(state)).toBe(true);
    expect(currentDocument(state).photo.stack.exposure.params.ev).toBe(0.42); // default

    // Redo
    state = redo(state);
    expect(state.index).toBe(1);
    expect(currentDocument(state).photo.stack.exposure.params.ev).toBe(1.5);
  });

  it('coalesces rapid parameter updates within coalesce window', () => {
    let state = createAppState();

    // Emulate a slider drag with 5 updates in rapid succession (<500ms)
    for (let i = 1; i <= 5; i++) {
      state = execute(
        state,
        {
          type: 'photo/setModuleParam',
          moduleId: 'exposure',
          patch: { ev: i * 0.2 },
        },
        { coalesceMs: 500 }
      );
    }

    // Should only have 2 entries: original + single coalesced drag
    expect(state.history).toHaveLength(2);
    expect(state.index).toBe(1);
    expect(currentDocument(state).photo.stack.exposure.params.ev).toBe(1.0);

    // One single undo should revert all 5 drag ticks back to original
    state = undo(state);
    expect(state.index).toBe(0);
    expect(currentDocument(state).photo.stack.exposure.params.ev).toBe(0.42);
  });

  it('truncates redo branch when new command lands on top of undo', () => {
    let state = createAppState();
    state = execute(state, {
      type: 'photo/setModuleParam',
      moduleId: 'exposure',
      patch: { ev: 1.0 },
    });
    state = execute(state, {
      type: 'photo/setModuleParam',
      moduleId: 'whiteBalance',
      patch: { temperatureK: 6500 },
    });

    expect(state.index).toBe(2);

    // Undo whiteBalance
    state = undo(state);
    expect(state.index).toBe(1);

    // Push new command (different module to test redo truncation)
    state = execute(state, {
      type: 'photo/setModuleParam',
      moduleId: 'filmicRgb',
      patch: { contrast: 1.8 },
    });

    // Redo branch should be discarded
    expect(state.index).toBe(2);
    expect(state.history).toHaveLength(3);
    expect(canRedo(state)).toBe(false);
    expect(currentDocument(state).photo.stack.filmicRgb.params.contrast).toBe(1.8);
  });

  it('supports time-travel via jumpTo', () => {
    let state = createAppState();
    state = execute(state, {
      type: 'photo/setModuleParam',
      moduleId: 'exposure',
      patch: { ev: 1.0 },
    });
    state = execute(state, {
      type: 'photo/setModuleParam',
      moduleId: 'whiteBalance',
      patch: { temperatureK: 6500 },
    });
    state = execute(state, {
      type: 'photo/toggleModule',
      moduleId: 'toneCurve',
      enabled: false,
    });

    expect(state.index).toBe(3);

    // Jump directly to state 1 (exposure edit)
    state = jumpTo(state, 1);
    expect(state.index).toBe(1);
    expect(currentDocument(state).photo.stack.exposure.params.ev).toBe(1.0);
    expect(currentDocument(state).photo.stack.toneCurve.enabled).toBe(true);

    // Jump to state 0 (original)
    state = jumpTo(state, 0);
    expect(state.index).toBe(0);
    expect(currentDocument(state).photo.stack.exposure.params.ev).toBe(0.42);
  });

  it('reports visible history in reverse chronological order', () => {
    let state = createAppState();
    state = execute(state, {
      type: 'photo/setModuleParam',
      moduleId: 'exposure',
      patch: { ev: 1.0 },
    });
    state = execute(state, {
      type: 'photo/setModuleParam',
      moduleId: 'whiteBalance',
      patch: { temperatureK: 6500 },
    });

    const visible = visibleHistory(state);
    expect(visible).toHaveLength(3);
    expect(visible[0].label).toBe('white balance');
    expect(visible[1].label).toBe('exposure');
    expect(visible[2].label).toBe('original');
  });

  it('respects history limit bound', () => {
    let state = createAppState();
    const limit = 5;

    for (let i = 0; i < 10; i++) {
      state = execute(
        state,
        {
          type: 'photo/setModuleParam',
          moduleId: 'exposure',
          patch: { ev: i },
        },
        { limit, coalesceMs: 0 }
      );
    }

    expect(state.history.length).toBeLessThanOrEqual(limit);
    expect(state.index).toBe(limit - 1);
  });
});
