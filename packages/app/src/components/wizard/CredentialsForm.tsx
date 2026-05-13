import { useMemo, useState } from 'react';

import type { BackendDraft } from '@trove/shared';
import { presetMetadataFor } from '@trove/collector-presets';

import { Button } from '../ui/index.js';

export type Kind = BackendDraft['kind'];

export interface CredentialsFormProps {
  kind: Kind;
  onSubmit: (draft: BackendDraft) => void;
  onBack: () => void;
}

function emptyDraft(kind: Kind): BackendDraft {
  switch (kind) {
    case 'signoz':
      return { kind, endpoint: 'ingest.us.signoz.cloud:443', ingestionKey: '' };
    case 'honeycomb':
      return { kind, team: '', dataset: '' };
    case 'grafana-cloud':
      return { kind, endpoint: '', auth: '' };
    case 'datadog':
      return { kind, site: 'datadoghq.com', apiKey: '' };
    case 'otlp-generic':
      return { kind, endpoint: '', protocol: 'http', headers: {} };
    case 'otelcol-passthrough':
      return { kind, endpoint: '' };
  }
}

function canonicalizeDraft(draft: BackendDraft): BackendDraft {
  if (draft.kind === 'signoz') {
    let endpoint = draft.endpoint
      .trim()
      .replace(/^https?:\/\//i, '')
      .replace(/\/+$/, '');
    if (endpoint && !/:\d+$/.test(endpoint)) {
      endpoint = `${endpoint}:443`;
    }
    return { ...draft, endpoint };
  }
  return draft;
}

function validate(draft: BackendDraft): string | null {
  switch (draft.kind) {
    case 'signoz': {
      const ep = draft.endpoint.trim();
      if (!ep) return 'Endpoint is required';
      if (!/^[A-Za-z0-9.-]+:\d+$/.test(ep)) {
        return 'Endpoint must look like ingest.us.signoz.cloud:443';
      }
      if (!draft.ingestionKey.trim()) return 'Ingestion key is required';
      return null;
    }
    case 'honeycomb':
      if (!draft.team.trim()) return 'Team API key is required';
      if (!draft.dataset.trim()) return 'Dataset is required';
      return null;
    case 'grafana-cloud':
      if (!isUrl(draft.endpoint)) return 'Endpoint must be a valid URL';
      if (!draft.auth.trim()) return 'Authorization header is required';
      return null;
    case 'datadog':
      if (!draft.site.trim()) return 'Site is required';
      if (!draft.apiKey.trim()) return 'API key is required';
      return null;
    case 'otlp-generic':
      if (!isUrl(draft.endpoint)) return 'Endpoint must be a valid URL';
      for (const [name, value] of Object.entries(draft.headers)) {
        if (!name.trim()) return 'Header names cannot be empty';
        if (!value.trim()) return `Header "${name}" needs a value`;
      }
      return null;
    case 'otelcol-passthrough':
      if (!isUrl(draft.endpoint)) return 'Endpoint must be a valid URL';
      return null;
  }
}

function isUrl(s: string): boolean {
  try {
    new URL(s);
    return true;
  } catch {
    return false;
  }
}

export function CredentialsForm({ kind, onSubmit, onBack }: CredentialsFormProps): JSX.Element {
  const [draft, setDraft] = useState<BackendDraft>(() => emptyDraft(kind));
  const [error, setError] = useState<string | null>(null);
  const meta = useMemo(() => presetMetadataFor(kind), [kind]);

  const handleSubmit = (e: React.FormEvent): void => {
    e.preventDefault();
    const canonical = canonicalizeDraft(draft);
    const message = validate(canonical);
    if (message !== null) {
      setError(message);
      return;
    }
    setError(null);
    onSubmit(canonical);
  };

  return (
    <section data-testid="credentials-form">
      <h2 className="text-[20px] font-semibold tracking-tight text-fg-primary dark:text-fg-primary-dark">
        {meta.label}
      </h2>
      <p className="mt-1 text-[13px] text-fg-secondary dark:text-fg-secondary-dark">
        {meta.description}
      </p>

      <form className="mt-6 space-y-4" onSubmit={handleSubmit}>
        {renderFields(draft, setDraft)}

        {error ? (
          <p data-testid="credentials-form-error" className="text-[13px] text-ios-red">
            {error}
          </p>
        ) : null}

        <div className="flex items-center justify-between pt-2">
          <Button variant="ghost" size="md" testid="credentials-form-back" onClick={onBack}>
            ← Back
          </Button>
          <Button variant="primary" size="md" testid="credentials-form-continue" type="submit">
            Continue
          </Button>
        </div>
      </form>
    </section>
  );
}

function renderFields(
  draft: BackendDraft,
  setDraft: React.Dispatch<React.SetStateAction<BackendDraft>>,
): JSX.Element {
  switch (draft.kind) {
    case 'signoz':
      return (
        <>
          <TextField
            label="OTLP endpoint"
            value={draft.endpoint}
            onChange={(v) => setDraft({ ...draft, endpoint: v })}
            placeholder="ingest.us.signoz.cloud:443"
            testId="signoz-endpoint"
            helper={
              <span>
                Copy this from SigNoz Cloud → Settings → Ingestion Settings. Either
                `https://ingest.us.signoz.cloud` or `ingest.us.signoz.cloud:443` works.
              </span>
            }
          />
          <PasswordField
            label="Ingestion key"
            value={draft.ingestionKey}
            onChange={(v) => setDraft({ ...draft, ingestionKey: v })}
            testId="signoz-ingestion-key"
          />
        </>
      );
    case 'honeycomb':
      return (
        <>
          <PasswordField
            label="Team API key"
            value={draft.team}
            onChange={(v) => setDraft({ ...draft, team: v })}
            testId="honeycomb-team"
          />
          <TextField
            label="Dataset"
            value={draft.dataset}
            onChange={(v) => setDraft({ ...draft, dataset: v })}
            placeholder="main"
            testId="honeycomb-dataset"
          />
        </>
      );
    case 'grafana-cloud':
      return (
        <>
          <TextField
            label="OTLP endpoint"
            value={draft.endpoint}
            onChange={(v) => setDraft({ ...draft, endpoint: v })}
            placeholder="https://otlp-gateway-prod-us-east-0.grafana.net/otlp"
            testId="grafana-cloud-endpoint"
          />
          <PasswordField
            label="Authorization header"
            value={draft.auth}
            onChange={(v) => setDraft({ ...draft, auth: v })}
            testId="grafana-cloud-auth"
            helper={
              <span>
                Paste the &quot;Basic &lt;token&gt;&quot; value from your Grafana Cloud OTel
                onboarding page.
              </span>
            }
          />
        </>
      );
    case 'datadog':
      return (
        <>
          <TextField
            label="Site"
            value={draft.site}
            onChange={(v) => setDraft({ ...draft, site: v })}
            placeholder="datadoghq.com"
            testId="datadog-site"
          />
          <PasswordField
            label="API key"
            value={draft.apiKey}
            onChange={(v) => setDraft({ ...draft, apiKey: v })}
            testId="datadog-api-key"
          />
        </>
      );
    case 'otlp-generic':
      return (
        <>
          <TextField
            label="Endpoint"
            value={draft.endpoint}
            onChange={(v) => setDraft({ ...draft, endpoint: v })}
            placeholder="https://otel.example.com"
            testId="otlp-generic-endpoint"
          />
          <fieldset>
            <legend className="text-[13px] font-medium text-fg-primary dark:text-fg-primary-dark">
              Protocol
            </legend>
            <div className="mt-2 flex gap-4">
              {(['http', 'grpc'] as const).map((p) => (
                <label
                  key={p}
                  className="flex items-center gap-2 text-[13px] text-fg-secondary dark:text-fg-secondary-dark"
                >
                  <input
                    type="radio"
                    name="otlp-protocol"
                    value={p}
                    checked={draft.protocol === p}
                    onChange={() => setDraft({ ...draft, protocol: p })}
                    data-testid={`otlp-generic-protocol-${p}`}
                  />
                  {p.toUpperCase()}
                </label>
              ))}
            </div>
          </fieldset>
          <HeadersEditor
            headers={draft.headers}
            onChange={(next) => setDraft({ ...draft, headers: next })}
          />
        </>
      );
    case 'otelcol-passthrough':
      return (
        <TextField
          label="Local collector endpoint"
          value={draft.endpoint}
          onChange={(v) => setDraft({ ...draft, endpoint: v })}
          placeholder="http://127.0.0.1:4318"
          testId="otelcol-passthrough-endpoint"
        />
      );
  }
}

const INPUT_CLASS =
  'mt-1 w-full rounded-[8px] border border-hairline bg-surface-elevated px-3 py-2 text-[13px] text-fg-primary placeholder:text-fg-tertiary focus:border-ios-blue focus:outline-none focus:ring-1 focus:ring-ios-blue dark:border-hairline-dark dark:bg-surface-elevated-dark dark:text-fg-primary-dark dark:placeholder:text-fg-tertiary-dark';

function TextField({
  label,
  value,
  onChange,
  placeholder,
  testId,
  helper,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  testId: string;
  helper?: React.ReactNode;
}): JSX.Element {
  return (
    <label className="block">
      <span className="text-[13px] font-medium text-fg-primary dark:text-fg-primary-dark">
        {label}
      </span>
      <input
        type="text"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        data-testid={testId}
        className={INPUT_CLASS}
      />
      {helper ? (
        <span className="mt-1 block text-[12px] text-fg-tertiary dark:text-fg-tertiary-dark">
          {helper}
        </span>
      ) : null}
    </label>
  );
}

function PasswordField({
  label,
  value,
  onChange,
  testId,
  helper,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  testId: string;
  helper?: React.ReactNode;
}): JSX.Element {
  const [reveal, setReveal] = useState(false);
  return (
    <label className="block">
      <span className="text-[13px] font-medium text-fg-primary dark:text-fg-primary-dark">
        {label}
      </span>
      <div className="mt-1 flex items-center gap-2">
        <input
          type={reveal ? 'text' : 'password'}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          data-testid={testId}
          className={`${INPUT_CLASS} mt-0`}
        />
        <Button
          variant="secondary"
          size="sm"
          testid={`${testId}-reveal`}
          onClick={() => setReveal((r) => !r)}
          aria-label={reveal ? 'Hide value' : 'Show value'}
        >
          {reveal ? 'Hide' : 'Show'}
        </Button>
      </div>
      {helper ? (
        <span className="mt-1 block text-[12px] text-fg-tertiary dark:text-fg-tertiary-dark">
          {helper}
        </span>
      ) : null}
    </label>
  );
}

interface HeadersEditorProps {
  headers: Record<string, string>;
  onChange: (next: Record<string, string>) => void;
}

function HeadersEditor({ headers, onChange }: HeadersEditorProps): JSX.Element {
  const entries = Object.entries(headers);

  const update = (oldName: string, newName: string, newValue: string): void => {
    const next: Record<string, string> = {};
    let inserted = false;
    for (const [name, value] of Object.entries(headers)) {
      if (name === oldName) {
        if (newName.trim()) {
          next[newName] = newValue;
          inserted = true;
        }
      } else {
        next[name] = value;
      }
    }
    if (!inserted && newName.trim()) {
      next[newName] = newValue;
    }
    onChange(next);
  };

  const remove = (name: string): void => {
    const next = { ...headers };
    delete next[name];
    onChange(next);
  };

  const addRow = (): void => {
    let i = 1;
    let name = 'header';
    while (Object.prototype.hasOwnProperty.call(headers, name)) {
      i += 1;
      name = `header${i}`;
    }
    onChange({ ...headers, [name]: '' });
  };

  const SMALL_INPUT =
    'flex-1 rounded-[8px] border border-hairline bg-surface-elevated px-3 py-1 text-[13px] text-fg-primary placeholder:text-fg-tertiary focus:border-ios-blue focus:outline-none focus:ring-1 focus:ring-ios-blue dark:border-hairline-dark dark:bg-surface-elevated-dark dark:text-fg-primary-dark dark:placeholder:text-fg-tertiary-dark';

  return (
    <fieldset>
      <legend className="text-[13px] font-medium text-fg-primary dark:text-fg-primary-dark">
        Auth headers
      </legend>
      <p className="mt-1 text-[12px] text-fg-tertiary dark:text-fg-tertiary-dark">
        Each header value is stored in your OS keychain, never in plain text on disk.
      </p>

      <ul className="mt-2 space-y-2" data-testid="otlp-generic-headers">
        {entries.length === 0 ? (
          <li className="text-[12px] italic text-fg-tertiary dark:text-fg-tertiary-dark">
            No headers yet.
          </li>
        ) : (
          entries.map(([name, value]) => (
            <li key={name} className="flex items-center gap-2">
              <input
                type="text"
                value={name}
                onChange={(e) => update(name, e.target.value, value)}
                placeholder="x-api-key"
                aria-label="Header name"
                data-testid={`otlp-generic-header-name-${name}`}
                className={SMALL_INPUT}
              />
              <input
                type="password"
                value={value}
                onChange={(e) => update(name, name, e.target.value)}
                placeholder="value"
                aria-label="Header value"
                data-testid={`otlp-generic-header-value-${name}`}
                className={SMALL_INPUT}
              />
              <Button
                variant="secondary"
                size="sm"
                testid={`otlp-generic-header-remove-${name}`}
                aria-label={`Remove ${name}`}
                onClick={() => remove(name)}
              >
                Remove
              </Button>
            </li>
          ))
        )}
      </ul>

      <button
        type="button"
        onClick={addRow}
        data-testid="otlp-generic-header-add"
        className="mt-2 rounded-[8px] border border-dashed border-hairline px-3 py-1 text-[12px] text-fg-secondary hover:border-ios-blue hover:text-ios-blue dark:border-hairline-dark dark:text-fg-secondary-dark"
      >
        + Add header
      </button>
    </fieldset>
  );
}
