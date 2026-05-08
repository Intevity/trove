import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { OverallHealthBadge } from './OverallHealthBadge.js';

describe('OverallHealthBadge', () => {
  it('renders the green state label', () => {
    render(<OverallHealthBadge health="green" />);
    const badge = screen.getByTestId('overall-health-badge');
    expect(badge.getAttribute('data-health')).toBe('green');
    expect(badge.textContent).toContain('Healthy');
  });

  it('renders the amber state label', () => {
    render(<OverallHealthBadge health="amber" />);
    const badge = screen.getByTestId('overall-health-badge');
    expect(badge.getAttribute('data-health')).toBe('amber');
    expect(badge.textContent).toContain('Awaiting telemetry');
  });

  it('renders the red state label', () => {
    render(<OverallHealthBadge health="red" />);
    const badge = screen.getByTestId('overall-health-badge');
    expect(badge.getAttribute('data-health')).toBe('red');
    expect(badge.textContent).toContain('Sidecar down');
  });

  it('renders the optional detail string when provided', () => {
    render(<OverallHealthBadge health="amber" detail="metrics endpoint unreachable" />);
    const badge = screen.getByTestId('overall-health-badge');
    expect(badge.textContent).toContain('metrics endpoint unreachable');
  });
});
