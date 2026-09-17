import type { BarrelRegionProps } from '@duckfoot/ui';
import { ActionButton, ChipGroup, MonoValue, PanelHeading, Stub, ToggleChip } from '@duckfoot/ui';
import { RAW_MODULE_ORDER } from '@duckfoot/core';
import type { RawModuleId, RawStack } from '@duckfoot/core';
import { usePhoto } from './PhotoContext';
import { Histogram } from './components/Histogram';
import { ModuleRow } from './components/ModuleRow';

export function PhotoInspector({ doc, dispatch }: BarrelRegionProps) {
  const { render, view, asset } = usePhoto();
  const stack = view.previewStack ?? doc.photo.stack;

  const previewParam = (moduleId: RawModuleId, patch: Record<string, unknown>) => {
    // Preview only — no command, so a drag does not fill the history with sixty
    // entries. The commit below is what lands on the stack.
    const base = view.previewStack ?? doc.photo.stack;
    const instance = base[moduleId];
    view.setPreviewStack({
      ...base,
      [moduleId]: { ...instance, params: { ...instance.params, ...patch } },
    } as RawStack);
  };

  const commitParam = (moduleId: RawModuleId, patch: Record<string, unknown>) => {
    view.setPreviewStack(null);
    dispatch({ type: 'photo/setModuleParam', moduleId, patch: patch as never });
  };

  return (
    <div className="df-inspector-body">
      <div className="df-scope">
        <PanelHeading
          trailing={
            <ChipGroup>
              {(['hist', 'wave', 'vect'] as const).map((id) => (
                <ToggleChip
                  key={id}
                  label={id.toUpperCase()}
                  active={view.scopeTab === id}
                  onClick={() => view.setScopeTab(id)}
                />
              ))}
            </ChipGroup>
          }
        >
          SCOPE
        </PanelHeading>
        <div className="df-scope__well">
          {view.scopeTab === 'hist' ? (
            <Histogram data={render.histogram} />
          ) : (
            <Stub
              label={view.scopeTab === 'wave' ? 'waveform' : 'vectorscope'}
              note="Not implemented. The histogram is computed from the same preview data."
            />
          )}
        </div>
      </div>

      <div className="df-inspector-tabs">
        {(['pipeline', 'masks', 'presets'] as const).map((tab) => (
          <button
            key={tab}
            type="button"
            className={`df-inspector-tab${view.inspectorTab === tab ? ' df-inspector-tab--active' : ''}`}
            onClick={() => view.setInspectorTab(tab)}
          >
            {tab[0].toUpperCase() + tab.slice(1)}
          </button>
        ))}
      </div>

      <div className="df-inspector-scroll">
        {view.inspectorTab === 'pipeline' ? (
          RAW_MODULE_ORDER.map((id) => (
            <ModuleRow
              key={id}
              id={id}
              stack={stack}
              expanded={view.expandedModule === id}
              stubbed={render.stubbedModules.includes(id)}
              onToggleExpanded={() => view.setExpandedModule(view.expandedModule === id ? null : id)}
              onToggleEnabled={(enabled) => dispatch({ type: 'photo/toggleModule', moduleId: id, enabled })}
              onPreviewParam={(patch) => previewParam(id, patch)}
              onCommitParam={(patch) => commitParam(id, patch)}
            />
          ))
        ) : view.inspectorTab === 'masks' ? (
          <Stub label="masks" note="Parametric and drawn masks are a later turn." />
        ) : (
          <Stub label="presets" note="Saved stacks are a later turn. Snapshots are in the Shelf." />
        )}
      </div>

      <div className="df-inspector-foot">
        <div className="df-inspector-foot__row">
          <MonoValue tone="dim">TIFF · 16-bit · ProPhoto</MonoValue>
          <MonoValue tone="dim">{estimateSize(render.sourceWidth, render.sourceHeight)}</MonoValue>
        </div>
        <div className="df-inspector-foot__actions">
          <ActionButton
            variant="primary"
            onClick={() => {
              const developedAsset = {
                id: `dev-${Date.now()}`,
                name: `${asset?.name.replace(/\.[^.]+$/, '') ?? 'photo'}-developed.tif`,
                type: 'photo' as const,
                url: '',
                size: 130_700_000,
                width: render.sourceWidth || 6000,
                height: render.sourceHeight || 4000,
                createdAt: Date.now(),
              };
              dispatch({ type: 'suite/addAssets', assets: [developedAsset] });
            }}
          >
            Develop to Shelf
          </ActionButton>
          <ActionButton disabled title="Sidecar export is not implemented yet">
            XMP
          </ActionButton>
        </div>
      </div>
    </div>
  );
}

/** 16-bit, three channels — the real arithmetic, not a fixed number from the board. */
function estimateSize(width: number, height: number): string {
  if (!width || !height) return '—';
  const bytes = width * height * 3 * 2;
  return `≈ ${Math.round(bytes / 1_000_000)} MB`;
}
