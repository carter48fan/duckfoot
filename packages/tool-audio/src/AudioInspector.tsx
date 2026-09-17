import type { BarrelRegionProps } from '@duckfoot/ui';
import { ActionButton, ChipGroup, MonoValue, PanelHeading, Slider, ToggleChip } from '@duckfoot/ui';
import {
  MAX_PHASE_OFFSET_SECONDS,
  formatTimecodeMs,
  orderedLanes,
  rampInMs,
  rampOutMs,
} from '@duckfoot/core';
import { useAudio } from './AudioContext';

export function AudioInspector({ doc, dispatch }: BarrelRegionProps) {
  const { view } = useAudio();
  const lanes = orderedLanes(doc.audio);
  const activeLane = lanes.find((l) => l.id === view.activeLaneId) ?? lanes[0] ?? null;
  const referenceLane = lanes.find((l) => l.isPhaseReference) ?? lanes[0] ?? null;

  const sel = doc.audio.selection;
  const inStr = sel ? formatTimecodeMs(sel.inSeconds) : '0:00.000';
  const outStr = sel ? formatTimecodeMs(sel.outSeconds) : '0:00.000';
  const lenStr = sel ? formatTimecodeMs(Math.max(0, sel.outSeconds - sel.inSeconds)) : '0:00.000';

  // Active lane phase preview / state
  const lanePhase = activeLane?.phase ?? {
    polarityInverted: false,
    offsetSeconds: 0,
    rotationDegrees: 0,
  };

  const previewOffset = activeLane ? view.previewOffsetSeconds[activeLane.id] : undefined;
  const effectiveOffsetSec = previewOffset ?? lanePhase.offsetSeconds;
  const offsetMs = effectiveOffsetSec * 1000;
  const offsetSmp = Math.round(effectiveOffsetSec * (doc.audio.sampleRate || 48000));
  const offsetDisplay = `${offsetMs > 0 ? '+' : ''}${offsetMs.toFixed(2)} ms · ${
    offsetSmp > 0 ? '+' : ''
  }${offsetSmp} smp`;

  const previewRotation = activeLane ? view.previewRotationDegrees[activeLane.id] : undefined;
  const effectiveRotation = previewRotation ?? lanePhase.rotationDegrees;
  const rotationDisplay = `${effectiveRotation > 0 ? '+' : ''}${Math.round(effectiveRotation)}°`;

  // Envelope readouts
  const envelope = doc.audio.envelope;
  const activeRampIn = envelope ? Math.round(rampInMs(envelope)) : 420;
  const activeRampOut = envelope ? Math.round(rampOutMs(envelope)) : 300;

  return (
    <div className="df-inspector-body">
      {/* Title */}
      <div
        style={{
          height: 'var(--df-section-header, 30px)',
          display: 'flex',
          alignItems: 'center',
          padding: '0 14px',
          borderBottom: '1px solid var(--df-rule-dim)',
          fontFamily: 'var(--df-font-serif)',
          fontSize: '14px',
          letterSpacing: '0.07em',
          color: 'var(--df-brass-label)',
        }}
      >
        {activeLane ? `SELECTION · ${activeLane.name.toUpperCase()}` : 'SELECTION'}
      </div>

      <div
        className="df-inspector-scroll"
        style={{ padding: '14px', display: 'flex', flexDirection: 'column', gap: '14px' }}
      >
        {/* Selection In / Out / Length */}
        <div style={{ display: 'flex', flexDirection: 'column', gap: '6px' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
            <span className="df-mono df-mono--dim" style={{ fontSize: '11px' }}>
              in
            </span>
            <span className="df-mono df-mono--in" style={{ fontSize: '11px' }}>
              {inStr}
            </span>
          </div>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
            <span className="df-mono df-mono--dim" style={{ fontSize: '11px' }}>
              out
            </span>
            <span className="df-mono df-mono--out" style={{ fontSize: '11px' }}>
              {outStr}
            </span>
          </div>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
            <span className="df-mono df-mono--dim" style={{ fontSize: '11px' }}>
              length
            </span>
            <span className="df-mono" style={{ fontSize: '11px', color: 'var(--df-text)' }}>
              {lenStr}
            </span>
          </div>
        </div>

        <div style={{ height: '1px', background: 'var(--df-rule-dim)' }} />

        {/* PHASE Panel */}
        <div style={{ display: 'flex', flexDirection: 'column', gap: '10px' }}>
          <PanelHeading
            trailing={
              referenceLane && activeLane && referenceLane.id !== activeLane.id ? (
                <span className="df-mono df-mono--accent">{`align to ${referenceLane.name}`}</span>
              ) : (
                <span className="df-mono df-mono--dim">reference lane</span>
              )
            }
          >
            PHASE
          </PanelHeading>

          {/* Invert polarity toggle box */}
          <button
            type="button"
            onClick={() => {
              if (!activeLane) return;
              dispatch({
                type: 'audio/setLanePhase',
                laneId: activeLane.id,
                patch: { polarityInverted: !lanePhase.polarityInverted },
              });
            }}
            style={{
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              padding: '8px 10px',
              border: lanePhase.polarityInverted
                ? '1px solid var(--df-brass-edge)'
                : '1px solid var(--df-control)',
              background: lanePhase.polarityInverted
                ? 'var(--df-wash-selection)'
                : 'transparent',
              cursor: 'pointer',
              textAlign: 'left',
            }}
          >
            <span style={{ fontSize: '11.5px', color: 'var(--df-text)' }}>Invert polarity</span>
            <span
              className="df-mono"
              style={{
                fontSize: '11px',
                color: lanePhase.polarityInverted ? 'var(--df-brass)' : 'var(--df-text4)',
              }}
            >
              {lanePhase.polarityInverted ? 'ø ON' : 'ø OFF'}
            </span>
          </button>

          {/* Time offset slider (bounded at ±50ms) */}
          <Slider
            label="Time offset"
            display={offsetDisplay}
            value={effectiveOffsetSec}
            min={-MAX_PHASE_OFFSET_SECONDS}
            max={MAX_PHASE_OFFSET_SECONDS}
            step={0.00005}
            bipolar={true}
            onPreview={(v) => {
              if (!activeLane) return;
              view.setPreviewOffsetSeconds({
                ...view.previewOffsetSeconds,
                [activeLane.id]: v,
              });
            }}
            onCommit={(v) => {
              if (!activeLane) return;
              const next = { ...view.previewOffsetSeconds };
              delete next[activeLane.id];
              view.setPreviewOffsetSeconds(next);
              dispatch({
                type: 'audio/setLanePhase',
                laneId: activeLane.id,
                patch: { offsetSeconds: v },
              });
            }}
          />

          {/* Phase rotation slider (−180..+180°) */}
          <Slider
            label="Phase rotation"
            display={rotationDisplay}
            value={effectiveRotation}
            min={-180}
            max={180}
            step={1}
            bipolar={true}
            onPreview={(v) => {
              if (!activeLane) return;
              view.setPreviewRotationDegrees({
                ...view.previewRotationDegrees,
                [activeLane.id]: v,
              });
            }}
            onCommit={(v) => {
              if (!activeLane) return;
              const next = { ...view.previewRotationDegrees };
              delete next[activeLane.id];
              view.setPreviewRotationDegrees(next);
              dispatch({
                type: 'audio/setLanePhase',
                laneId: activeLane.id,
                patch: { rotationDegrees: Math.round(v) },
              });
            }}
          />

          {/* Correlation readout */}
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              padding: '8px 10px',
              border: '1px solid var(--df-control)',
              background: 'var(--df-well)',
            }}
          >
            <span style={{ fontSize: '11px', color: 'var(--df-text-calm)' }}>Correlation</span>
            <MonoValue tone="in">+0.76 in phase</MonoValue>
          </div>

          {/* Auto-align button */}
          <ActionButton
            onClick={() => {
              if (!activeLane) return;
              // Align nudge
              dispatch({
                type: 'audio/setLanePhase',
                laneId: activeLane.id,
                patch: { offsetSeconds: -0.00142, rotationDegrees: 38 },
              });
            }}
          >
            {referenceLane ? `Auto-align to ${referenceLane.name}` : 'Auto-align to ref'}
          </ActionButton>
        </div>

        <div style={{ height: '1px', background: 'var(--df-rule-dim)' }} />

        {/* GAIN OVER SELECTION Section */}
        <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
          <PanelHeading>GAIN OVER SELECTION</PanelHeading>
          <ChipGroup>
            {(['lin', 'log', 's-curve'] as const).map((interp) => (
              <ToggleChip
                key={interp}
                label={interp.toUpperCase()}
                active={(doc.audio.envelope?.interpolation ?? 'lin') === interp}
                onClick={() =>
                  dispatch({ type: 'audio/setEnvelopeInterpolation', interpolation: interp })
                }
              />
            ))}
          </ChipGroup>
          <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: '11px' }}>
            <span className="df-mono df-mono--dim">Ramp in</span>
            <span className="df-mono">{`${activeRampIn} ms`}</span>
          </div>
          <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: '11px' }}>
            <span className="df-mono df-mono--dim">Ramp out</span>
            <span className="df-mono">{`${activeRampOut} ms`}</span>
          </div>
        </div>
      </div>

      {/* Footer export actions */}
      <div className="df-inspector-foot">
        <div className="df-inspector-foot__actions" style={{ flexDirection: 'column', gap: '6px' }}>
          <ActionButton
            variant="primary"
            onClick={() => {
              // Mixdown WAV export landing back in the Shelf!
              const mixAsset = {
                id: `mix-${Date.now()}`,
                name: 'stems-mixdown.wav',
                type: 'audio' as const,
                url: '',
                size: 36_864_000,
                duration: view.totalDurationSeconds,
                createdAt: Date.now(),
              };
              dispatch({ type: 'suite/addAssets', assets: [mixAsset] });
            }}
          >
            Export mixdown WAV
          </ActionButton>
          <ActionButton
            onClick={() => {
              const stemAssets = lanes.map((l) => ({
                id: `stem-${l.name}-${Date.now()}`,
                name: `${l.name}-edited.wav`,
                type: 'audio' as const,
                url: '',
                size: 12_288_000,
                duration: view.totalDurationSeconds,
                createdAt: Date.now(),
              }));
              dispatch({ type: 'suite/addAssets', assets: stemAssets });
            }}
          >
            Export stems
          </ActionButton>
        </div>
      </div>
    </div>
  );
}
