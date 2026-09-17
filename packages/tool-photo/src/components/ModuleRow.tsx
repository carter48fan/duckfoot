import type { RawModuleId, RawModuleInstance } from '@duckfoot/core';
import { RAW_MODULE_META, moduleSummary } from '@duckfoot/core';
import type { RawStack } from '@duckfoot/core';
import { Slider, ToggleChip, ChipGroup, Stub } from '@duckfoot/ui';
import { ToneCurveEditor } from './ToneCurveEditor';

/**
 * Ranges for the numeric parameters, which are a UI concern rather than a document
 * one — the document stores a value, not the extent of a slider.
 */
const RANGES: Record<string, { min: number; max: number; step: number; unit?: string; bipolar?: boolean }> = {
  'rawLevels.black': { min: 0, max: 16383, step: 1 },
  'rawLevels.white': { min: 0, max: 65535, step: 1 },
  'whiteBalance.temperatureK': { min: 1800, max: 12000, step: 10, unit: ' K' },
  'whiteBalance.tint': { min: -100, max: 100, step: 1, bipolar: true },
  'exposure.ev': { min: -5, max: 5, step: 0.01, unit: ' EV', bipolar: true },
  'exposure.blackLevel': { min: -0.1, max: 0.1, step: 0.001, bipolar: true },
  'filmicRgb.whiteRelEv': { min: 0, max: 8, step: 0.1 },
  'filmicRgb.blackRelEv': { min: -16, max: 0, step: 0.1 },
  'filmicRgb.latitude': { min: 0, max: 100, step: 1, unit: '%' },
  'filmicRgb.contrast': { min: 0.5, max: 3, step: 0.01 },
  'localContrast.detail': { min: -100, max: 100, step: 1, bipolar: true },
  'localContrast.radiusPx': { min: 1, max: 64, step: 1, unit: ' px' },
  'captureSharpen.radiusPx': { min: 0.1, max: 4, step: 0.1, unit: ' px' },
  'captureSharpen.amount': { min: 0, max: 2, step: 0.01 },
  'denoiseProfiled.luma': { min: 0, max: 100, step: 1 },
  'denoiseProfiled.chroma': { min: 0, max: 100, step: 1 },
  'lensCorrection.distortion': { min: -100, max: 100, step: 1, bipolar: true },
  'lensCorrection.vignetting': { min: -100, max: 100, step: 1, bipolar: true },
  'lensCorrection.ca': { min: -100, max: 100, step: 1, bipolar: true },
  'cropRotate.angleDeg': { min: -45, max: 45, step: 0.1, unit: '°', bipolar: true },
};

const ENUMS: Record<string, readonly string[]> = {
  'demosaic.algorithm': ['rcd', 'ppg', 'bilinear'],
  'outputProfile.profile': ['srgb', 'display-p3', 'prophoto', 'rec2020'],
  'toneCurve.channel': ['rgb', 'r', 'g', 'b', 'l'],
};

export interface ModuleRowProps {
  id: RawModuleId;
  stack: RawStack;
  expanded: boolean;
  stubbed: boolean;
  onToggleExpanded: () => void;
  onToggleEnabled: (enabled: boolean) => void;
  onPreviewParam: (patch: Record<string, unknown>) => void;
  onCommitParam: (patch: Record<string, unknown>) => void;
}

/**
 * One row of the pipeline, rendered from the module registry rather than hand-written.
 *
 * All fourteen rows come from this component. The readout on the right is
 * `summary()` from @duckfoot/core, which is why no unit string appears in this file
 * except as slider metadata — and why the HISTORY panel can never disagree with the
 * module list about what a module is called.
 */
export function ModuleRow({
  id,
  stack,
  expanded,
  stubbed,
  onToggleExpanded,
  onToggleEnabled,
  onPreviewParam,
  onCommitParam,
}: ModuleRowProps) {
  const meta = RAW_MODULE_META[id];
  const instance = stack[id] as RawModuleInstance;
  const enabled = instance.enabled;
  const params = instance.params as Record<string, unknown>;

  return (
    <div className={`df-module${expanded ? ' df-module--expanded' : ''}`}>
      <div className="df-module__head">
        <button
          type="button"
          className={`df-module__dot${enabled ? ' df-module__dot--on' : ''}`}
          onClick={() => onToggleEnabled(!enabled)}
          title={enabled ? `Disable ${meta.label}` : `Enable ${meta.label}`}
          aria-pressed={enabled}
        />
        <button
          type="button"
          className={`df-module__label${enabled ? '' : ' df-module__label--off'}`}
          onClick={onToggleExpanded}
          aria-expanded={expanded}
        >
          {meta.label}
        </button>
        {stubbed && enabled ? (
          <span className="df-module__badge" title="No implementation yet — this module is skipped">
            skipped
          </span>
        ) : null}
        <span className={`df-mono df-module__summary${enabled ? '' : ' df-module__summary--off'}`}>
          {moduleSummary(stack, id)}
        </span>
      </div>

      {expanded ? (
        <div className="df-module__body">
          {stubbed ? (
            <Stub label="not implemented" note={`${meta.label} has no implementation yet, so the pipeline skips it.`} />
          ) : null}

          {id === 'toneCurve' ? (
            <ToneCurveEditor
              nodes={(params.nodes as never) ?? []}
              onPreview={(nodes) => onPreviewParam({ nodes })}
              onCommit={(nodes) => onCommitParam({ nodes })}
            />
          ) : null}

          {Object.entries(params).map(([key, value]) => {
            const path = `${id}.${key}`;
            const enumOptions = ENUMS[path];
            if (enumOptions) {
              return (
                <ChipGroup key={key}>
                  {enumOptions.map((option) => (
                    <ToggleChip
                      key={option}
                      label={option.toUpperCase()}
                      active={value === option}
                      onClick={() => onCommitParam({ [key]: option })}
                    />
                  ))}
                </ChipGroup>
              );
            }

            const range = RANGES[path];
            if (typeof value === 'number' && range) {
              return (
                <Slider
                  key={key}
                  label={key}
                  display={`${formatNumber(value, range.step)}${range.unit ?? ''}`}
                  value={value}
                  min={range.min}
                  max={range.max}
                  step={range.step}
                  bipolar={range.bipolar}
                  onPreview={(next) => onPreviewParam({ [key]: next })}
                  onCommit={(next) => onCommitParam({ [key]: next })}
                />
              );
            }
            return null;
          })}
        </div>
      ) : null}
    </div>
  );
}

function formatNumber(value: number, step: number): string {
  const decimals = step >= 1 ? 0 : step >= 0.1 ? 1 : step >= 0.01 ? 2 : 3;
  const text = Math.abs(value).toFixed(decimals);
  return value < 0 ? `−${text}` : text;
}
