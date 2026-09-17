import { useCallback, useState } from 'react';

/**
 * Panel collapse, remembered across sessions.
 *
 * DESIGN.md: "Shelf and Inspector are collapsible and remember their state."
 * Boards 2a and 2b do not draw the collapsed form — board 1a does, as a 30px rail
 * with a vertical label — so the mock is incomplete here rather than in conflict.
 *
 * localStorage access is wrapped because it throws outright in a private window
 * with site data blocked, and a panel preference is not worth a blank screen.
 */
export function usePanelState(key: string, initial: boolean): [boolean, () => void] {
  const [open, setOpen] = useState(() => {
    try {
      const stored = localStorage.getItem(key);
      return stored === null ? initial : stored === '1';
    } catch {
      return initial;
    }
  });

  const toggle = useCallback(() => {
    setOpen((previous) => {
      const next = !previous;
      try {
        localStorage.setItem(key, next ? '1' : '0');
      } catch {
        // Preference is lost, the panel still works.
      }
      return next;
    });
  }, [key]);

  return [open, toggle];
}
