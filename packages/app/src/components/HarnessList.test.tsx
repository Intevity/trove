import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import type { DetectedHarness } from '@trove/shared';

import { HarnessList } from './HarnessList.js';

function row(overrides: Partial<DetectedHarness>): DetectedHarness {
  return {
    id: 'claude-code',
    detected: true,
    configPath: '/home/me/.claude/settings.json',
    telemetry: 'off',
    detectionMethod: 'config-dir',
    troveRegionPresent: false,
    ...overrides,
  };
}

describe('HarnessList', () => {
  it('renders a loading state while the detection sweep runs', () => {
    render(<HarnessList harnesses={[]} loading />);
    expect(screen.getByTestId('harness-list-loading')).toBeDefined();
  });

  it('renders an empty state when nothing was detected', () => {
    render(<HarnessList harnesses={[]} loading={false} />);
    expect(screen.getByTestId('harness-list-empty')).toBeDefined();
  });

  it('renders one row per detected harness', () => {
    render(
      <HarnessList
        harnesses={[
          row({ id: 'claude-code' }),
          row({ id: 'gemini-cli', detected: false, detectionMethod: null, configPath: null }),
        ]}
        loading={false}
      />,
    );
    expect(screen.getByTestId('harness-row-claude-code')).toBeDefined();
    expect(screen.getByTestId('harness-row-gemini-cli')).toBeDefined();
  });

  it('shows the config path when detected via config-dir', () => {
    render(<HarnessList harnesses={[row({})]} loading={false} />);
    expect(screen.getByText(/\.claude\/settings\.json/)).toBeDefined();
  });

  it('disables the toggle for harnesses whose adapter is not yet implemented', () => {
    render(
      <HarnessList
        harnesses={[
          row({
            id: 'codex-cli',
            configPath: '/home/me/.codex/config.toml',
          }),
        ]}
        loading={false}
      />,
    );
    const toggle = screen.getByLabelText('toggle-codex-cli') as HTMLButtonElement;
    expect(toggle.disabled).toBe(true);
    expect(toggle.textContent).toMatch(/Sprint 4/);
  });

  it('disables the toggle when the harness was not detected', () => {
    render(
      <HarnessList
        harnesses={[
          row({
            id: 'claude-code',
            detected: false,
            detectionMethod: null,
            configPath: null,
          }),
        ]}
        loading={false}
      />,
    );
    const toggle = screen.getByLabelText('toggle-claude-code') as HTMLButtonElement;
    expect(toggle.disabled).toBe(true);
  });

  it('enables the toggle for an available adapter on a detected harness', () => {
    render(<HarnessList harnesses={[row({ id: 'claude-code' })]} loading={false} />);
    const toggle = screen.getByLabelText('toggle-claude-code') as HTMLButtonElement;
    expect(toggle.disabled).toBe(false);
    expect(toggle.textContent).toBe('Enable');
  });

  it('enables the gemini-cli toggle (Sprint 3 PR 3)', () => {
    render(
      <HarnessList
        harnesses={[row({ id: 'gemini-cli', configPath: '/home/me/.gemini/settings.json' })]}
        loading={false}
      />,
    );
    const toggle = screen.getByLabelText('toggle-gemini-cli') as HTMLButtonElement;
    expect(toggle.disabled).toBe(false);
    expect(toggle.textContent).toBe('Enable');
  });

  it('shows Disable label when troveRegionPresent is true', () => {
    render(
      <HarnessList
        harnesses={[row({ id: 'claude-code', troveRegionPresent: true })]}
        loading={false}
      />,
    );
    const toggle = screen.getByLabelText('toggle-claude-code') as HTMLButtonElement;
    expect(toggle.textContent).toBe('Disable');
  });

  it('calls onEnable when an Enable click fires', () => {
    const onEnable = vi.fn();
    render(
      <HarnessList harnesses={[row({ id: 'claude-code' })]} loading={false} onEnable={onEnable} />,
    );
    fireEvent.click(screen.getByLabelText('toggle-claude-code'));
    expect(onEnable).toHaveBeenCalledWith('claude-code');
  });

  it('calls onDisable when a Disable click fires', () => {
    const onDisable = vi.fn();
    render(
      <HarnessList
        harnesses={[row({ id: 'claude-code', troveRegionPresent: true })]}
        loading={false}
        onDisable={onDisable}
      />,
    );
    fireEvent.click(screen.getByLabelText('toggle-claude-code'));
    expect(onDisable).toHaveBeenCalledWith('claude-code');
  });

  it('shows the busy label when the row is mid-revert', () => {
    render(
      <HarnessList
        harnesses={[row({ id: 'claude-code', troveRegionPresent: true })]}
        loading={false}
        busyIds={new Set(['claude-code'])}
      />,
    );
    const toggle = screen.getByLabelText('toggle-claude-code') as HTMLButtonElement;
    expect(toggle.disabled).toBe(true);
    expect(toggle.textContent).toBe('Disabling…');
  });

  it('renders the telemetry status label for each row', () => {
    render(
      <HarnessList
        harnesses={[
          row({ id: 'claude-code', telemetry: 'on' }),
          row({ id: 'gemini-cli', telemetry: 'off' }),
          row({
            id: 'qwen-code',
            telemetry: 'unknown',
            detected: false,
            detectionMethod: null,
            configPath: null,
          }),
        ]}
        loading={false}
      />,
    );
    expect(screen.getByTestId('harness-telemetry-claude-code').textContent).toBe('Telemetry on');
    expect(screen.getByTestId('harness-telemetry-gemini-cli').textContent).toBe('Telemetry off');
    expect(screen.getByTestId('harness-telemetry-qwen-code').textContent).toBe('Telemetry unknown');
  });

  it('falls back to a generic detected label when method is unknown', () => {
    render(
      <HarnessList
        harnesses={[
          row({
            id: 'claude-code',
            detectionMethod: 'path-binary',
            configPath: null,
          }),
        ]}
        loading={false}
      />,
    );
    expect(screen.getByText('Detected on PATH')).toBeDefined();
  });
});
