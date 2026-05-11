import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { SYNTHETIC_SERVICE_NAME, SYNTHETIC_SPAN_NAME, SYNTHETIC_TRACE_ID } from '@trove/shared';

import { SyntheticSpanHints } from './SyntheticSpanHints.js';

describe('SyntheticSpanHints', () => {
  beforeEach(() => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', {
      value: { writeText },
      configurable: true,
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('renders the three canary facts', () => {
    render(<SyntheticSpanHints />);
    expect(screen.getByTestId('hint-service-name').textContent).toBe(SYNTHETIC_SERVICE_NAME);
    expect(screen.getByTestId('hint-span-name').textContent).toBe(SYNTHETIC_SPAN_NAME);
    expect(screen.getByTestId('hint-trace-id').textContent).toBe(SYNTHETIC_TRACE_ID);
  });

  it('uses the backend label in the preamble when known', () => {
    render(<SyntheticSpanHints backendKind="signoz" />);
    expect(screen.getByTestId('synthetic-span-hints').textContent).toContain('SigNoz Cloud');
  });

  it('falls back to generic phrasing when backend kind is omitted', () => {
    render(<SyntheticSpanHints />);
    expect(screen.getByTestId('synthetic-span-hints').textContent).toContain(
      'your observability backend',
    );
  });

  it('copies the value to the clipboard and flips the button label', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', {
      value: { writeText },
      configurable: true,
    });
    render(<SyntheticSpanHints />);
    fireEvent.click(screen.getByTestId('hint-trace-id-copy'));
    await waitFor(() => {
      expect(writeText).toHaveBeenCalledWith(SYNTHETIC_TRACE_ID);
    });
    await waitFor(() => {
      expect(screen.getByTestId('hint-trace-id-copy').textContent).toBe('copied');
    });
  });

  it('renders a tooltip on the trace id explaining the fixed canary', () => {
    render(<SyntheticSpanHints />);
    expect(screen.getByTestId('hint-trace-id').getAttribute('title')).toContain('canary');
  });
});
