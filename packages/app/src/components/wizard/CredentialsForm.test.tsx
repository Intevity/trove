import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { CredentialsForm } from './CredentialsForm.js';

describe('CredentialsForm', () => {
  it('submits a valid SigNoz draft', () => {
    const onSubmit = vi.fn();
    render(<CredentialsForm kind="signoz" onSubmit={onSubmit} onBack={vi.fn()} />);

    // Region pre-filled with 'us-east'; user adds the ingestion key.
    fireEvent.change(screen.getByTestId('signoz-ingestion-key'), {
      target: { value: 'sk-ingest-test' },
    });
    fireEvent.click(screen.getByTestId('credentials-form-continue'));

    expect(onSubmit).toHaveBeenCalledWith({
      kind: 'signoz',
      region: 'us-east',
      ingestionKey: 'sk-ingest-test',
    });
  });

  it('blocks submit when a required field is empty', () => {
    const onSubmit = vi.fn();
    render(<CredentialsForm kind="signoz" onSubmit={onSubmit} onBack={vi.fn()} />);
    fireEvent.click(screen.getByTestId('credentials-form-continue'));
    expect(screen.getByTestId('credentials-form-error').textContent).toMatch(/Ingestion key/);
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it('toggles password fields between password and text type', () => {
    render(<CredentialsForm kind="datadog" onSubmit={vi.fn()} onBack={vi.fn()} />);
    const input = screen.getByTestId('datadog-api-key') as HTMLInputElement;
    expect(input.type).toBe('password');
    fireEvent.click(screen.getByTestId('datadog-api-key-reveal'));
    expect(input.type).toBe('text');
    fireEvent.click(screen.getByTestId('datadog-api-key-reveal'));
    expect(input.type).toBe('password');
  });

  it('rejects a non-URL endpoint for grafana-cloud', () => {
    const onSubmit = vi.fn();
    render(<CredentialsForm kind="grafana-cloud" onSubmit={onSubmit} onBack={vi.fn()} />);
    fireEvent.change(screen.getByTestId('grafana-cloud-endpoint'), {
      target: { value: 'not-a-url' },
    });
    fireEvent.change(screen.getByTestId('grafana-cloud-auth'), {
      target: { value: 'Basic xxx' },
    });
    fireEvent.click(screen.getByTestId('credentials-form-continue'));
    expect(screen.getByTestId('credentials-form-error').textContent).toMatch(/valid URL/);
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it('lets the user add and remove headers in the otlp-generic form', () => {
    const onSubmit = vi.fn();
    render(<CredentialsForm kind="otlp-generic" onSubmit={onSubmit} onBack={vi.fn()} />);

    // Endpoint is required.
    fireEvent.change(screen.getByTestId('otlp-generic-endpoint'), {
      target: { value: 'https://otel.example.com' },
    });

    // Add two headers.
    fireEvent.click(screen.getByTestId('otlp-generic-header-add'));
    fireEvent.click(screen.getByTestId('otlp-generic-header-add'));

    // The default placeholder names are 'header' and 'header2'. Edit
    // their names + values.
    fireEvent.change(screen.getByTestId('otlp-generic-header-name-header'), {
      target: { value: 'x-api-key' },
    });
    fireEvent.change(screen.getByTestId('otlp-generic-header-value-x-api-key'), {
      target: { value: 'sk-test' },
    });
    fireEvent.change(screen.getByTestId('otlp-generic-header-name-header2'), {
      target: { value: 'x-trace-id' },
    });
    fireEvent.change(screen.getByTestId('otlp-generic-header-value-x-trace-id'), {
      target: { value: 'trace-1' },
    });

    // Remove one header — verify it's gone.
    fireEvent.click(screen.getByTestId('otlp-generic-header-remove-x-trace-id'));

    fireEvent.click(screen.getByTestId('credentials-form-continue'));
    expect(onSubmit).toHaveBeenCalledWith({
      kind: 'otlp-generic',
      endpoint: 'https://otel.example.com',
      protocol: 'http',
      headers: { 'x-api-key': 'sk-test' },
    });
  });

  it('flags an empty header value before submit', () => {
    const onSubmit = vi.fn();
    render(<CredentialsForm kind="otlp-generic" onSubmit={onSubmit} onBack={vi.fn()} />);
    fireEvent.change(screen.getByTestId('otlp-generic-endpoint'), {
      target: { value: 'https://otel.example.com' },
    });
    fireEvent.click(screen.getByTestId('otlp-generic-header-add'));
    fireEvent.change(screen.getByTestId('otlp-generic-header-name-header'), {
      target: { value: 'x-api-key' },
    });
    // Note: no value typed.
    fireEvent.click(screen.getByTestId('credentials-form-continue'));
    expect(screen.getByTestId('credentials-form-error').textContent).toMatch(/needs a value/);
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it('submits the otelcol-passthrough kind without secrets', () => {
    const onSubmit = vi.fn();
    render(<CredentialsForm kind="otelcol-passthrough" onSubmit={onSubmit} onBack={vi.fn()} />);
    fireEvent.change(screen.getByTestId('otelcol-passthrough-endpoint'), {
      target: { value: 'http://127.0.0.1:4318' },
    });
    fireEvent.click(screen.getByTestId('credentials-form-continue'));
    expect(onSubmit).toHaveBeenCalledWith({
      kind: 'otelcol-passthrough',
      endpoint: 'http://127.0.0.1:4318',
    });
  });

  it('calls onBack when the back button is clicked', () => {
    const onBack = vi.fn();
    render(<CredentialsForm kind="signoz" onSubmit={vi.fn()} onBack={onBack} />);
    fireEvent.click(screen.getByTestId('credentials-form-back'));
    expect(onBack).toHaveBeenCalled();
  });
});
