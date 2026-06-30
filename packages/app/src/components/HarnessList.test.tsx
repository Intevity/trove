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
          row({ id: 'antigravity-cli', detected: false, detectionMethod: null, configPath: null }),
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
      'harness-row-antigravity-cli',
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
          row({ id: 'antigravity-cli', detected: false, detectionMethod: null, configPath: null }),
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
    const antigravityLogo = screen.getByTestId('harness-logo-antigravity-cli');
    expect(antigravityLogo.getAttribute('class')).toMatch(/grayscale/);
    expect(antigravityLogo.getAttribute('class')).toMatch(/opacity-/);
  });

  it('renders one row per detected harness', () => {
    render(
      <HarnessList
        harnesses={[
          row({ id: 'claude-code' }),
          row({ id: 'antigravity-cli', detected: false, detectionMethod: null, configPath: null }),
        ]}
        loading={false}
      />,
    );
    expect(screen.getByTestId('harness-row-claude-code')).toBeDefined();
    expect(screen.getByTestId('harness-row-antigravity-cli')).toBeDefined();
  });

  it('shows the config path when detected via config-dir', () => {
    render(<HarnessList harnesses={[row({})]} loading={false} />);
    expect(screen.getByText(/\.claude\/settings\.json/)).toBeDefined();
  });

  it('renders no toggle for harnesses whose adapter is not yet implemented', () => {
    // Sprint 7 PR 3: gating is driven by the IPC-side adapterAvailable
    // bool. When no adapter is available and no setup guide is
    // registered, Trove shows no right-side action at all — the row
    // is detection-only.
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
    expect(screen.queryByLabelText('toggle-cline')).toBeNull();
    expect(screen.queryByLabelText('setup-cline')).toBeNull();
    expect(screen.queryByText('Adapter not yet available')).toBeNull();
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

  it('enables the antigravity-cli toggle (Sprint 3 PR 3)', () => {
    render(
      <HarnessList
        harnesses={[
          row({ id: 'antigravity-cli', configPath: '/home/me/.gemini/antigravity-cli/hooks.json' }),
        ]}
        loading={false}
      />,
    );
    const toggle = screen.getByLabelText('toggle-antigravity-cli') as HTMLButtonElement;
    expect(toggle.disabled).toBe(false);
    expect(toggle.textContent).toBe('Enable');
  });

  it('shows Disable label when the id is in enabledIds', () => {
    render(
      <HarnessList
        harnesses={[row({ id: 'claude-code' })]}
        loading={false}
        enabledIds={new Set(['claude-code'])}
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
        harnesses={[row({ id: 'claude-code' })]}
        loading={false}
        enabledIds={new Set(['claude-code'])}
        onDisable={onDisable}
      />,
    );
    fireEvent.click(screen.getByLabelText('toggle-claude-code'));
    expect(onDisable).toHaveBeenCalledWith('claude-code');
  });

  it('shows the busy label when the row is mid-revert', () => {
    render(
      <HarnessList
        harnesses={[row({ id: 'claude-code' })]}
        loading={false}
        enabledIds={new Set(['claude-code'])}
        busyIds={new Set(['claude-code'])}
      />,
    );
    const toggle = screen.getByLabelText('toggle-claude-code') as HTMLButtonElement;
    expect(toggle.disabled).toBe(true);
    expect(toggle.textContent).toBe('Disabling…');
  });

  it('treats troveRegionPresent as irrelevant to the button label — only AppState drives it', () => {
    // Cursor IDE and Cursor CLI share `~/.cursor/hooks.json`, so the
    // filesystem-level `troveRegionPresent` flag goes true for BOTH
    // when either is enabled. The button must source its label from
    // `enabledIds` (the persisted `AppState.harnesses` set) instead.
    render(
      <HarnessList
        harnesses={[
          row({ id: 'cursor-ide', troveRegionPresent: true }),
          row({ id: 'cursor-cli', troveRegionPresent: true }),
        ]}
        loading={false}
        enabledIds={new Set(['cursor-ide'])}
      />,
    );
    expect((screen.getByLabelText('toggle-cursor-ide') as HTMLButtonElement).textContent).toBe(
      'Disable',
    );
    expect((screen.getByLabelText('toggle-cursor-cli') as HTMLButtonElement).textContent).toBe(
      'Enable',
    );
  });

  it('renders the telemetry status label for each row', () => {
    render(
      <HarnessList
        harnesses={[
          row({ id: 'claude-code', telemetry: 'on' }),
          row({ id: 'antigravity-cli', telemetry: 'off' }),
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
    expect(screen.getByTestId('harness-telemetry-antigravity-cli').textContent).toBe(
      'Telemetry off',
    );
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
    expect(screen.queryByTestId('harness-badge-cursor-ide')).toBeNull();
  });

  it('enables the cursor-cli toggle and surfaces a Partial Coverage badge with the hooks-docs link in its popover', () => {
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
    const badge = screen.getByTestId('harness-badge-cursor-cli');
    expect(badge.textContent).toContain('Partial Coverage');
    // The popover content (description + docs link) is revealed on hover.
    fireEvent.mouseEnter(badge);
    const link = screen.getByRole('link') as HTMLAnchorElement;
    expect(link.href).toBe('https://cursor.com/docs/hooks');
    expect(link.target).toBe('_blank');
    expect(badge.textContent).toMatch(/beforeShellExecution/);
  });

  it('enables the opencode toggle and renders no advisory badge', () => {
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
    expect(screen.queryByTestId('harness-badge-opencode')).toBeNull();
  });

  it('does not surface an advisory badge when opencode is undetected either', () => {
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
    expect(screen.queryByTestId('harness-badge-opencode')).toBeNull();
  });

  it('shows a Best Effort badge on the cline row with an OpenTelemetry-context popover', () => {
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
    const badge = screen.getByTestId('harness-badge-cline');
    expect(badge.textContent).toContain('Best Effort');
    fireEvent.mouseEnter(badge);
    expect(badge.textContent).toMatch(/OpenTelemetry/);
    const link = screen.getByRole('link') as HTMLAnchorElement;
    expect(link.href.startsWith('https://')).toBe(true);
    expect(link.target).toBe('_blank');
  });

  it('shows a Best Effort badge on the aider row mentioning the shell-rc wrapper', () => {
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
    const badge = screen.getByTestId('harness-badge-aider');
    expect(badge.textContent).toContain('Best Effort');
    fireEvent.mouseEnter(badge);
    expect(badge.textContent).toMatch(/shell-rc/);
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

  it('shows a Best Effort badge on the copilot-cli row that mentions the gh-copilot rename', () => {
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
    const badge = screen.getByTestId('harness-badge-copilot-cli');
    expect(badge.textContent).toContain('Best Effort');
    fireEvent.mouseEnter(badge);
    expect(badge.textContent).toMatch(/gh-copilot/);
  });

  it('renders an Enable toggle and an Auto-detected badge for claude-desktop', () => {
    render(
      <HarnessList
        harnesses={[
          row({
            id: 'claude-desktop',
            adapterAvailable: true,
            detected: true,
            detectionMethod: 'app-bundle',
            configPath: null,
          }),
        ]}
        loading={false}
      />,
    );
    // The legacy "setup" affordance is gone — every adapter-backed
    // harness uses the standard toggle.
    expect(screen.queryByLabelText('setup-claude-desktop')).toBeNull();
    const toggle = screen.getByLabelText('toggle-claude-desktop') as HTMLButtonElement;
    expect(toggle.disabled).toBe(false);
    expect(['Enable', 'Disable']).toContain(toggle.textContent);
    // The audit-log explanation now sits inside the Auto-detected
    // popover and reveals on hover.
    const badge = screen.getByTestId('harness-badge-claude-desktop');
    expect(badge.textContent).toContain('Auto-detected');
    fireEvent.mouseEnter(badge);
    expect(badge.textContent).toMatch(/audit log/i);
  });

  it('does not render a coverage badge on harnesses without one', () => {
    render(<HarnessList harnesses={[row({ id: 'claude-code' })]} loading={false} />);
    expect(screen.queryByTestId('harness-badge-claude-code')).toBeNull();
  });

  it('tags harnesses absent from the validation matrix with a Beta pill', () => {
    render(
      <HarnessList
        harnesses={[
          row({
            id: 'junie-cli',
            detected: true,
            detectionMethod: 'path-binary',
            configPath: null,
            adapterAvailable: false,
          }),
          row({ id: 'claude-code', detected: true }),
          row({
            id: 'sentinel',
            detected: true,
            detectionMethod: 'path-binary',
            configPath: null,
            adapterAvailable: false,
          }),
        ]}
        loading={false}
      />,
    );
    // junie-cli is absent from the harness × platform matrix → Beta.
    expect(screen.getByTestId('harness-beta-junie-cli')).toBeDefined();
    // claude-code is fully validated → no Beta pill.
    expect(screen.queryByTestId('harness-beta-claude-code')).toBeNull();
    // sentinel was validated manually → no longer Beta.
    expect(screen.queryByTestId('harness-beta-sentinel')).toBeNull();
  });

  it('retitles the Sentinel row and links out to its repo', () => {
    render(
      <HarnessList
        harnesses={[
          row({
            id: 'sentinel',
            detected: true,
            detectionMethod: 'path-binary',
            configPath: null,
            adapterAvailable: false,
          }),
        ]}
        loading={false}
      />,
    );
    expect(screen.getByText('Sentinel - A Claude Code companion')).toBeDefined();
    const link = screen.getByTestId('harness-learn-more-sentinel') as HTMLAnchorElement;
    expect(link.getAttribute('href')).toBe('https://github.com/Intevity/sentinel');
    expect(link.getAttribute('target')).toBe('_blank');
    expect(link.getAttribute('rel')).toBe('noreferrer noopener');
  });

  it('renders no button for an adapterless harness with no setup guide', () => {
    render(
      <HarnessList
        harnesses={[
          row({
            id: 'qwen-code',
            adapterAvailable: false,
            detected: true,
            detectionMethod: 'path-binary',
            configPath: null,
          }),
        ]}
        loading={false}
      />,
    );
    expect(screen.queryByLabelText('toggle-qwen-code')).toBeNull();
    expect(screen.queryByLabelText('setup-qwen-code')).toBeNull();
    expect(screen.queryByText('Adapter not yet available')).toBeNull();
  });
});
