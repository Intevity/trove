import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { Toast } from './Toast.js';

describe('Toast', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('renders the message and dismiss button', () => {
    render(<Toast message="restart Cursor" onDismiss={() => {}} />);
    expect(screen.getByTestId('toast').textContent).toContain('restart Cursor');
    expect(screen.getByTestId('toast-dismiss')).toBeDefined();
  });

  it('auto-dismisses after the default 15s', () => {
    const onDismiss = vi.fn();
    render(<Toast message="hi" onDismiss={onDismiss} />);
    act(() => {
      vi.advanceTimersByTime(14_999);
    });
    expect(onDismiss).not.toHaveBeenCalled();
    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it('respects a custom duration', () => {
    const onDismiss = vi.fn();
    render(<Toast message="hi" durationMs={500} onDismiss={onDismiss} />);
    act(() => {
      vi.advanceTimersByTime(500);
    });
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it('does not auto-dismiss when durationMs is 0', () => {
    const onDismiss = vi.fn();
    render(<Toast message="hi" durationMs={0} onDismiss={onDismiss} />);
    act(() => {
      vi.advanceTimersByTime(60_000);
    });
    expect(onDismiss).not.toHaveBeenCalled();
  });

  it('fires onDismiss when the user clicks the X', () => {
    const onDismiss = vi.fn();
    render(<Toast message="hi" onDismiss={onDismiss} />);
    fireEvent.click(screen.getByTestId('toast-dismiss'));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it('cancels the auto-dismiss timer when unmounted', () => {
    const onDismiss = vi.fn();
    const { unmount } = render(<Toast message="hi" onDismiss={onDismiss} />);
    unmount();
    act(() => {
      vi.advanceTimersByTime(60_000);
    });
    expect(onDismiss).not.toHaveBeenCalled();
  });
});
