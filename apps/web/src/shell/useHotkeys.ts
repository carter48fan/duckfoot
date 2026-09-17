import { useEffect } from 'react';

export interface HotkeyHandlers {
  onUndo: () => void;
  onRedo: () => void;
  onTogglePlay: () => void;
  onSetIn: () => void;
  onSetOut: () => void;
  onAutoAlign: () => void;
  onDeleteRegion: () => void;
  onEscape: () => void;
}

/**
 * Does the event come from somewhere a keystroke means text, not a command?
 *
 * DESIGN.md: "Hotkeys never fire while a text field has focus. Check the event
 * target — for every hotkey, not just Space." The previous Audio barrel guarded
 * Space with `e.target === document.body` and left `i` and `o` completely open, so
 * typing a filename set in- and out-points. One guard, one place, all keys.
 */
function isTextEntry(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  return /^(input|textarea|select)$/i.test(target.tagName);
}

export function useHotkeys(handlers: HotkeyHandlers) {
  useEffect(() => {
    const handle = (event: KeyboardEvent) => {
      if (isTextEntry(event.target)) return;

      const meta = event.metaKey || event.ctrlKey;
      if (meta && event.key.toLowerCase() === 'z') {
        event.preventDefault();
        if (event.shiftKey) handlers.onRedo();
        else handlers.onUndo();
        return;
      }
      if (meta) return;

      switch (event.key) {
        case ' ':
          event.preventDefault();
          handlers.onTogglePlay();
          break;
        case 'i':
        case 'I':
          handlers.onSetIn();
          break;
        case 'o':
        case 'O':
          handlers.onSetOut();
          break;
        case 'A':
          if (event.shiftKey) handlers.onAutoAlign();
          break;
        case 'Backspace':
        case 'Delete':
          event.preventDefault();
          handlers.onDeleteRegion();
          break;
        case 'Escape':
          handlers.onEscape();
          break;
        default:
          break;
      }
    };

    window.addEventListener('keydown', handle);
    return () => window.removeEventListener('keydown', handle);
  }, [handlers]);
}
