import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { App } from './App.js';

describe('App', () => {
  it('renders the Hello, Trove header', () => {
    render(<App />);
    const header = screen.getByTestId('app-header');
    expect(header.textContent).toBe('Hello, Trove');
  });

  it('reports the MVP harness count', () => {
    render(<App />);
    expect(screen.getByText(/Targeting/i)).toBeTruthy();
  });
});
