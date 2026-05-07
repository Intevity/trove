import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { PresetPicker } from './PresetPicker.js';

describe('PresetPicker', () => {
  it('renders one card per preset kind', () => {
    render(<PresetPicker onSelect={vi.fn()} />);
    expect(screen.getByTestId('preset-signoz')).toBeDefined();
    expect(screen.getByTestId('preset-honeycomb')).toBeDefined();
    expect(screen.getByTestId('preset-grafana-cloud')).toBeDefined();
    expect(screen.getByTestId('preset-datadog')).toBeDefined();
    expect(screen.getByTestId('preset-otlp-generic')).toBeDefined();
    expect(screen.getByTestId('preset-otelcol-passthrough')).toBeDefined();
  });

  it('places SigNoz first and tags it Recommended', () => {
    const { container } = render(<PresetPicker onSelect={vi.fn()} />);
    const buttons = container.querySelectorAll('button[data-testid^="preset-"]');
    expect(buttons[0]?.getAttribute('data-testid')).toBe('preset-signoz');
    expect(screen.getAllByTestId('preset-recommended-badge')).toHaveLength(1);
  });

  it('calls onSelect with the chosen kind', () => {
    const onSelect = vi.fn();
    render(<PresetPicker onSelect={onSelect} />);
    fireEvent.click(screen.getByTestId('preset-honeycomb'));
    expect(onSelect).toHaveBeenCalledWith('honeycomb');
  });
});
