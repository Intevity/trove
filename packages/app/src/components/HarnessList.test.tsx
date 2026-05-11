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
    adapterAvailable: true,
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

  it('orders detected harnesses before undetected ones, preserving relative order within each group', () => {
    render(
      <HarnessList
        harnesses={[
          row({ id: 'gemini-cli', detected: false, detectionMethod: null, configPath: null }),
          row({ id: 'claude-code', detected: true }),
          row({ id: 'cline', detected: false, detectionMethod: null, configPath: null }),
          row({ id: 'aider', detected: true }),
        ]}
        loading={false}
      />,
    );
    const rows = screen.getAllByTestId(/^harness-row-/);
    expect(rows.map((r) => r.getAttribute('data-testid'))).toEqual([
      'harness-row-claude-code',
      'harness-row-aider',
      'harness-row-gemini-cli',
      'harness-row-cline',
    ]);
  });

  it('flags each row with a data-detected attribute the styles key off of', () => {
    render(
      <HarnessList
        harnesses={[
          row({ id: 'claude-code', detected: true }),
          row({ id: 'qwen-code', detected: false, detectionMethod: null, configPath: null }),
        ]}
        loading={false}
      />,
    );
    expect(screen.getByTestId('harness-row-claude-code').getAttribute('data-detected')).toBe(
      'true',
    );
    expect(screen.getByTestId('harness-row-qwen-code').getAttribute('data-detected')).toBe('false');
  });

  it('renders an inline SVG logo for every harness row, transparent root', () => {
    render(
      <HarnessList
        harnesses={[
          row({ id: 'claude-code', detected: true }),
          row({ id: 'gemini-cli', detected: false, detectionMethod: null, configPath: null }),
        ]}
        loading={false}
      />,
    );
    const claudeLogo = screen.getByTestId('harness-logo-claude-code');
    expect(claudeLogo.tagName).toBe('svg');
    // Transparent background: the SVG element itself carries no fill.
    expect(claudeLogo.getAttribute('fill')).toBeNull();
    // Detected logo renders without the dimming filter classes.
    expect(claudeLogo.getAttribute('class')).not.toMatch(/grayscale/);
    // Undetected logo is dimmed.
    const geminiLogo = screen.getByTestId('harness-logo-gemini-cli');
    expect(geminiLogo.getAttribute('class')).toMatch(/grayscale/);
    expect(geminiLogo.getAttribute('class')).toMatch(/opacity-/);
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
    // Sprint 7 PR 3: gating is now driven by the IPC-side adapterAvailable
    // bool. Tier 3 harnesses (cline / aider / copilot-cli) report false
    // until Sprint 9 wires their adapters.
    render(
      <HarnessList
        harnesses={[
          row({
            id: 'cline',
            adapterAvailable: false,
          }),
        ]}
        loading={false}
      />,
    );
    const toggle = screen.getByLabelText('toggle-cline') as HTMLButtonElement;
    expect(toggle.disabled).toBe(true);
    expect(toggle.textContent).toBe('Adapter not yet available');
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

  it('enables the cursor-ide toggle (Sprint 7 PR 1) without an advisory note', () => {
    render(
      <HarnessList
        harnesses={[
          row({
            id: 'cursor-ide',
            configPath: '/home/me/.cursor/hooks.json',
            detectionMethod: 'config-dir',
          }),
        ]}
        loading={false}
      />,
    );
    const toggle = screen.getByLabelText('toggle-cursor-ide') as HTMLButtonElement;
    expect(toggle.disabled).toBe(false);
    expect(toggle.textContent).toBe('Enable');
    expect(screen.queryByTestId('harness-coverage-note-cursor-ide')).toBeNull();
  });

  it('enables the cursor-cli toggle (Sprint 7 PR 1) and shows the partial-coverage advisory', () => {
    render(
      <HarnessList
        harnesses={[
          row({
            id: 'cursor-cli',
            configPath: '/home/me/.cursor/hooks.json',
            detectionMethod: 'path-binary',
          }),
        ]}
        loading={false}
      />,
    );
    const toggle = screen.getByLabelText('toggle-cursor-cli') as HTMLButtonElement;
    expect(toggle.disabled).toBe(false);
    expect(toggle.textContent).toBe('Enable');
    const advisory = screen.getByTestId('harness-coverage-note-cursor-cli') as HTMLAnchorElement;
    expect(advisory.textContent).toBe('Partial event coverage');
    // Sprint 7 PR 3: the advisory is a hyperlink to Cursor's hooks docs
    // and carries a tooltip explaining the coverage gap.
    expect(advisory.href).toBe('https://cursor.com/docs/hooks');
    expect(advisory.title).toContain('beforeShellExecution');
    expect(advisory.target).toBe('_blank');
  });

  it('enables the opencode toggle (Sprint 7 PR 2) and shows no advisory note', () => {
    render(
      <HarnessList
        harnesses={[
          row({
            id: 'opencode',
            configPath: '/home/me/.config/opencode/opencode.json',
            detectionMethod: 'config-dir',
          }),
        ]}
        loading={false}
      />,
    );
    const toggle = screen.getByLabelText('toggle-opencode') as HTMLButtonElement;
    expect(toggle.disabled).toBe(false);
    expect(toggle.textContent).toBe('Enable');
    expect(screen.queryByTestId('harness-coverage-note-opencode')).toBeNull();
  });

  it('does not surface a coverage advisory when opencode is undetected either', () => {
    render(
      <HarnessList
        harnesses={[
          row({
            id: 'opencode',
            detected: false,
            detectionMethod: null,
            configPath: null,
          }),
        ]}
        loading={false}
      />,
    );
    expect(screen.queryByTestId('harness-coverage-note-opencode')).toBeNull();
  });

  it('shows a best-effort advisory on the cline row (Sprint 9 PR 1)', () => {
    render(
      <HarnessList
        harnesses={[
          row({
            id: 'cline',
            adapterAvailable: false,
            detected: true,
            detectionMethod: 'config-dir',
          }),
        ]}
        loading={false}
      />,
    );
    const advisory = screen.getByTestId('harness-coverage-note-cline') as HTMLAnchorElement;
    expect(advisory.textContent).toBe('Best-effort coverage');
    expect(advisory.title).toContain('OpenTelemetry');
    expect(advisory.href.startsWith('https://')).toBe(true);
    expect(advisory.target).toBe('_blank');
  });

  it('shows a best-effort advisory on the aider row (Sprint 9 PR 1)', () => {
    render(
      <HarnessList
        harnesses={[
          row({
            id: 'aider',
            adapterAvailable: false,
            detected: true,
            detectionMethod: 'path-binary',
            configPath: null,
          }),
        ]}
        loading={false}
      />,
    );
    const advisory = screen.getByTestId('harness-coverage-note-aider') as HTMLAnchorElement;
    expect(advisory.textContent).toBe('Best-effort coverage');
    expect(advisory.title).toContain('shell-rc');
  });

  it('renders a Refresh button in the header when onRefresh is provided, hidden otherwise', () => {
    const onRefresh = vi.fn();
    const { rerender } = render(
      <HarnessList harnesses={[row({})]} loading={false} onRefresh={onRefresh} />,
    );
    expect(screen.getByTestId('harness-list-refresh')).toBeDefined();
    rerender(<HarnessList harnesses={[row({})]} loading={false} />);
    expect(screen.queryByTestId('harness-list-refresh')).toBeNull();
  });

  it('invokes onRefresh when the Refresh button is clicked', () => {
    const onRefresh = vi.fn();
    render(<HarnessList harnesses={[row({})]} loading={false} onRefresh={onRefresh} />);
    fireEvent.click(screen.getByTestId('harness-list-refresh'));
    expect(onRefresh).toHaveBeenCalled();
  });

  it('disables and re-labels the Refresh button while a sweep is in flight', () => {
    const onRefresh = vi.fn();
    render(<HarnessList harnesses={[]} loading={true} onRefresh={onRefresh} />);
    const btn = screen.getByTestId('harness-list-refresh') as HTMLButtonElement;
    expect(btn.disabled).toBe(true);
    expect(btn.textContent).toContain('Refreshing');
  });

  it('keeps the Refresh button visible while the empty state is shown', () => {
    const onRefresh = vi.fn();
    render(<HarnessList harnesses={[]} loading={false} onRefresh={onRefresh} />);
    expect(screen.getByTestId('harness-list-empty')).toBeDefined();
    expect(screen.getByTestId('harness-list-refresh')).toBeDefined();
  });

  it('shows a best-effort advisory on the copilot-cli row that mentions the gh-copilot rename', () => {
    render(
      <HarnessList
        harnesses={[
          row({
            id: 'copilot-cli',
            adapterAvailable: false,
            detected: true,
            detectionMethod: 'path-binary',
            configPath: null,
          }),
        ]}
        loading={false}
      />,
    );
    const advisory = screen.getByTestId('harness-coverage-note-copilot-cli') as HTMLAnchorElement;
    expect(advisory.textContent).toBe('Best-effort coverage');
    expect(advisory.title).toContain('gh-copilot');
  });
});
