import { describe, expect, it } from 'vitest';
import { createRawStack, RAW_MODULE_ORDER } from '@duckfoot/core';
import { createScene, renderRawStack } from '../src';

function createCleanStack() {
  const stack = createRawStack();
  for (const id of RAW_MODULE_ORDER) {
    stack[id].enabled = false;
  }
  return stack;
}

describe('RAW Photo Pipeline & Modules', () => {
  it('preserves input scene data immutability during render', () => {
    const input = createScene(4, 4);
    for (let i = 0; i < input.data.length; i++) {
      input.data[i] = 0.25;
    }

    const stack = createCleanStack();
    // Enable exposure with +1 EV (double brightness)
    stack.exposure = {
      id: 'exposure',
      enabled: true,
      params: { ev: 1, blackLevel: 0 },
    };

    const { scene: developed, stubbedModules } = renderRawStack(input, stack, { quality: 'preview' });

    // Input buffer must be unchanged
    for (let i = 0; i < input.data.length; i++) {
      expect(input.data[i]).toBe(0.25);
    }

    // Output buffer reflects exposure gain (0.25 * 2^1 = 0.5)
    expect(developed.data[0]).toBeCloseTo(0.5, 4);
    expect(stubbedModules).toEqual([]);
  });

  it('correctly applies white balance multipliers', () => {
    const input = createScene(1, 1);
    input.data[0] = 0.5;
    input.data[1] = 0.5;
    input.data[2] = 0.5;

    const stack = createCleanStack();
    stack.whiteBalance = {
      id: 'whiteBalance',
      enabled: true,
      params: { temperatureK: 6500, tint: 0 },
    };

    const { scene: developed } = renderRawStack(input, stack, { quality: 'preview' });

    // Higher Kelvin should increase red gain relative to 5500K base
    const rGain = Math.pow(6500 / 5500, 0.55);
    const bGain = Math.pow(5500 / 6500, 0.55);
    expect(developed.data[0]).toBeCloseTo(0.5 * rGain, 4);
    expect(developed.data[1]).toBeCloseTo(0.5, 4);
    expect(developed.data[2]).toBeCloseTo(0.5 * bGain, 4);
  });

  it('reports stubbed modules without crashing the pipeline', () => {
    const input = createScene(2, 2);
    const stack = createCleanStack();
    // Enable a stub module like 'toneEqualizer' or 'captureSharpen'
    stack.toneEqualizer = {
      id: 'toneEqualizer',
      enabled: true,
      params: { bandsDb: [0, 0, 0, 0, 0, 0, 0, 0] },
    };
    stack.captureSharpen = {
      id: 'captureSharpen',
      enabled: true,
      params: { radiusPx: 0.7, amount: 0.5 },
    };

    const { scene: developed, stubbedModules } = renderRawStack(input, stack, { quality: 'preview' });
    expect(developed).toBeDefined();
    expect(stubbedModules).toContain('toneEqualizer');
    expect(stubbedModules).toContain('captureSharpen');
  });

  it('correctly processes filmic RGB without negative values or hard highlight clipping', () => {
    const input = createScene(3, 1);
    input.data[0] = 0.05; // shadow
    input.data[1] = 0.18; // midtone
    input.data[2] = 2.5;  // extreme highlight with 2+ stops headroom

    const stack = createCleanStack();
    stack.filmicRgb = {
      id: 'filmicRgb',
      enabled: true,
      params: {
        whiteRelEv: 3.5,
        blackRelEv: -8,
        latitude: 2.0,
        contrast: 1.1,
      },
    };

    const { scene: developed } = renderRawStack(input, stack, { quality: 'preview' });
    expect(developed.colorspace).toBe('display');
    // All values must be bounded in [0, 1]
    for (let i = 0; i < developed.data.length; i++) {
      expect(developed.data[i]).toBeGreaterThanOrEqual(0);
      expect(developed.data[i]).toBeLessThanOrEqual(1);
    }
    // Extreme highlight must have rolled off smoothly without blowing to NaN or infinity
    expect(developed.data[2]).toBeGreaterThan(0.8);
  });
});
