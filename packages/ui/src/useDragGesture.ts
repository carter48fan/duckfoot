import { useCallback, useRef } from 'react';

/**
 * The single implementation of DESIGN.md's "Drag has a live preview and a cancel":
 *
 *   "Crop boxes, trim handles, clip moves: show the result during the drag, revert
 *    on Esc."
 *
 * Used by in/out handles, the playhead, envelope nodes, gain and phase sliders, the
 * tone-curve nodes and the A/B divider. Writing it once is what keeps Esc working
 * everywhere instead of in the three places someone remembered.
 *
 * The shape matters for undo: `onMove` previews without dispatching, and `onCommit`
 * fires once on pointerup. Combined with command coalescing in @duckfoot/core, a
 * drag produces exactly one history entry.
 */
export interface DragGestureOptions<T> {
  /** Captures whatever the gesture needs to compute deltas and to revert. */
  onStart: (event: React.PointerEvent) => T;
  /** Live preview. Called on every pointermove; must not dispatch a command. */
  onMove: (state: T, event: PointerEvent) => void;
  /** Fires once, on pointerup. This is where the command is dispatched. */
  onCommit: (state: T, event: PointerEvent) => void;
  /** Esc, or a cancelled pointer. Restore whatever onStart captured. */
  onCancel?: (state: T) => void;
}

export function useDragGesture<T>(options: DragGestureOptions<T>) {
  const optionsRef = useRef(options);
  optionsRef.current = options;

  return useCallback((event: React.PointerEvent) => {
    // Ignore secondary buttons: a right-click drag should open a menu, not edit.
    if (event.button !== 0) return;
    event.preventDefault();

    const state = optionsRef.current.onStart(event);
    let cancelled = false;

    const handleMove = (moveEvent: PointerEvent) => {
      if (cancelled) return;
      optionsRef.current.onMove(state, moveEvent);
    };

    const finish = (upEvent: PointerEvent) => {
      cleanup();
      if (cancelled) return;
      optionsRef.current.onCommit(state, upEvent);
    };

    const cancel = () => {
      if (cancelled) return;
      cancelled = true;
      optionsRef.current.onCancel?.(state);
      cleanup();
    };

    const handleKey = (keyEvent: KeyboardEvent) => {
      if (keyEvent.key === 'Escape') {
        keyEvent.preventDefault();
        cancel();
      }
    };

    function cleanup() {
      window.removeEventListener('pointermove', handleMove);
      window.removeEventListener('pointerup', finish);
      window.removeEventListener('pointercancel', cancel);
      window.removeEventListener('keydown', handleKey, true);
    }

    window.addEventListener('pointermove', handleMove);
    window.addEventListener('pointerup', finish);
    window.addEventListener('pointercancel', cancel);
    // Capture phase: Esc must reach us before any dialog or panel handler sees it.
    window.addEventListener('keydown', handleKey, true);
  }, []);
}
