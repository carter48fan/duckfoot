import type { ReactNode } from 'react';
import { useDragGesture } from './useDragGesture';

/**
 * Shared chrome primitives.
 *
 * Every interactive element here is a real <button> or <input>, not the styled
 * <div> the design boards use. The boards are static images and have no focus or
 * hover states to show; DESIGN.md's interaction laws are keyboard-first, so the
 * implementation needs both. The visual delta is a brass focus ring that appears
 * only on keyboard focus.
 */

/** Instrument Serif section heading — SHELF, SCOPE, PHASE, CORRELATION. */
export function PanelHeading({ children, trailing }: { children: ReactNode; trailing?: ReactNode }) {
  return (
    <div className="df-panel-heading">
      <span className="df-panel-heading__label">{children}</span>
      {trailing ? <span className="df-panel-heading__trailing">{trailing}</span> : null}
    </div>
  );
}

/**
 * A numeric readout.
 *
 * DESIGN.md: "Monospace for all numbers — timecode, dimensions, sample rate,
 * percentages. They are read as values, not prose."
 */
export function MonoValue({ children, tone = 'default' }: { children: ReactNode; tone?: 'default' | 'accent' | 'dim' | 'in' | 'out' }) {
  return <span className={`df-mono df-mono--${tone}`}>{children}</span>;
}

/**
 * Marks a control whose implementation does not exist.
 *
 * Renders visibly rather than silently doing nothing, because the alternative is a
 * control the user moves with no effect and no explanation — the failure invariant 7
 * is written to prevent.
 */
export function Stub({ label, note }: { label: string; note?: string }) {
  return (
    <div className="df-stub" role="note">
      <span className="df-stub__label">{label}</span>
      {note ? <span className="df-stub__note">{note}</span> : null}
    </div>
  );
}

export interface ToggleChipProps {
  label: string;
  active: boolean;
  onClick: () => void;
  disabled?: boolean;
  title?: string;
}

/**
 * The segmented chip that appears six times across the two boards: LIN/LOG/S-CURVE,
 * HIST/WAVE/VECT, RGB/R/G/B/L, CLIP/FOCUS/GAMUT, Lighttable/Develop, A/B.
 */
export function ToggleChip({ label, active, onClick, disabled, title }: ToggleChipProps) {
  return (
    <button
      type="button"
      className={`df-chip${active ? ' df-chip--active' : ''}`}
      onClick={onClick}
      disabled={disabled}
      title={title}
      aria-pressed={active}
    >
      {label}
    </button>
  );
}

export function ChipGroup({ children }: { children: ReactNode }) {
  return <div className="df-chip-group">{children}</div>;
}

export interface SliderProps {
  label: string;
  /** Formatted for display — always shown, per "no mystery sliders". */
  display: string;
  value: number;
  min: number;
  max: number;
  step?: number;
  /** Live preview during a drag. Must not dispatch a command. */
  onPreview: (value: number) => void;
  /** Once, on release. This is where the command goes. */
  onCommit: (value: number) => void;
  /** Draws the fill from the centre rather than the left — for bipolar controls. */
  bipolar?: boolean;
  disabled?: boolean;
}

/**
 * A slider that shows its value.
 *
 * DESIGN.md: "Every control that changes the media shows its current numeric value.
 * No mystery sliders." The `display` prop is required for that reason — a caller
 * cannot render one without deciding how its number reads.
 */
export function Slider({
  label,
  display,
  value,
  min,
  max,
  step = 0.01,
  onPreview,
  onCommit,
  bipolar = false,
  disabled = false,
}: SliderProps) {
  const range = max - min;
  const fraction = range === 0 ? 0 : (value - min) / range;
  const zeroFraction = range === 0 ? 0 : (0 - min) / range;

  const fillLeft = bipolar ? Math.min(fraction, zeroFraction) : 0;
  const fillRight = bipolar ? Math.max(fraction, zeroFraction) : fraction;

  const startDrag = useDragGesture<{ startX: number; startValue: number; width: number }>({
    onStart: (event) => {
      const groove = event.currentTarget as HTMLElement;
      return {
        startX: event.clientX,
        startValue: value,
        width: groove.getBoundingClientRect().width || 1,
      };
    },
    onMove: (state, event) => {
      const delta = ((event.clientX - state.startX) / state.width) * range;
      onPreview(clampStep(state.startValue + delta, min, max, step));
    },
    onCommit: (state, event) => {
      const delta = ((event.clientX - state.startX) / state.width) * range;
      onCommit(clampStep(state.startValue + delta, min, max, step));
    },
    onCancel: (state) => onPreview(state.startValue),
  });

  return (
    <div className={`df-slider${disabled ? ' df-slider--disabled' : ''}`}>
      <div className="df-slider__row">
        <span className="df-slider__label">{label}</span>
        <MonoValue>{display}</MonoValue>
      </div>
      <div
        className="df-slider__track"
        onPointerDown={disabled ? undefined : startDrag}
        role="slider"
        tabIndex={disabled ? -1 : 0}
        aria-label={label}
        aria-valuenow={value}
        aria-valuemin={min}
        aria-valuemax={max}
        aria-valuetext={display}
        onKeyDown={(event) => {
          if (disabled) return;
          const nudge = event.shiftKey ? step * 10 : step;
          if (event.key === 'ArrowLeft' || event.key === 'ArrowDown') {
            event.preventDefault();
            onCommit(clampStep(value - nudge, min, max, step));
          } else if (event.key === 'ArrowRight' || event.key === 'ArrowUp') {
            event.preventDefault();
            onCommit(clampStep(value + nudge, min, max, step));
          }
        }}
      >
        {bipolar ? <span className="df-slider__centre" /> : null}
        <span
          className="df-slider__fill"
          style={{ left: `${fillLeft * 100}%`, right: `${(1 - fillRight) * 100}%` }}
        />
        <span className="df-slider__handle" style={{ left: `${fraction * 100}%` }} />
      </div>
    </div>
  );
}

function clampStep(value: number, min: number, max: number, step: number): number {
  const stepped = Math.round(value / step) * step;
  // Re-round after clamping: floating-point steps such as 0.01 accumulate error
  // that shows up as 0.30000000000000004 in a readout.
  return Number(Math.min(max, Math.max(min, stepped)).toFixed(6));
}

export interface IconButtonProps {
  glyph: string;
  label: string;
  active?: boolean;
  onClick?: () => void;
  disabled?: boolean;
  tone?: 'default' | 'accent' | 'destructive';
}

/**
 * The boards use text glyphs for every affordance — ⟳ ⇄ ø ▶ ■ ⌫ M S — so there is
 * no icon dependency anywhere in the two rewritten barrels.
 */
export function IconButton({ glyph, label, active, onClick, disabled, tone = 'default' }: IconButtonProps) {
  return (
    <button
      type="button"
      className={`df-icon-button df-icon-button--${tone}${active ? ' df-icon-button--active' : ''}`}
      onClick={onClick}
      disabled={disabled}
      title={label}
      aria-label={label}
      aria-pressed={active}
    >
      <span aria-hidden="true">{glyph}</span>
    </button>
  );
}

export function ActionButton({
  children,
  onClick,
  variant = 'default',
  disabled,
  title,
}: {
  children: ReactNode;
  onClick?: () => void;
  variant?: 'default' | 'primary' | 'destructive';
  disabled?: boolean;
  title?: string;
}) {
  return (
    <button
      type="button"
      className={`df-action df-action--${variant}`}
      onClick={onClick}
      disabled={disabled}
      title={title}
    >
      {children}
    </button>
  );
}
