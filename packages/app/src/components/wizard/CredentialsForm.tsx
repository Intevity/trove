import { useMemo, useState } from 'react';

import type { BackendDraft } from '@trove/shared';
import { presetMetadataFor } from '@trove/collector-presets';

export type Kind = BackendDraft['kind'];

export interface CredentialsFormProps {
  kind: Kind;
  /** Called when the form is filled in and the user clicks Continue. */
  onSubmit: (draft: BackendDraft) => void;
  /** Called when the user clicks Back to return to the preset picker. */
  onBack: () => void;
}

/** Initial draft skeleton for the chosen kind. Populated by the form
 *  fields below; submitted to the parent only when validation passes. */
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

/** Normalize fields that users commonly paste in inconvenient forms.
 *  SigNoz Cloud's docs sometimes show the ingestion endpoint as a full
 *  `https://...` URL, but the OTLP/gRPC exporter expects `host:port`.
 *  Strip the scheme and any trailing slashes here so a paste-and-go
 *  flow works. */
function canonicalizeDraft(draft: BackendDraft): BackendDraft {
  if (draft.kind === 'signoz') {
    const endpoint = draft.endpoint
      .trim()
      .replace(/^https?:\/\//i, '')
      .replace(/\/+$/, '');
    return { ...draft, endpoint };
  }
  return draft;
}

/** Per-kind validator. Returns the first user-facing error string, or
 *  null when the draft is ready to submit. */
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
      <h2 className="text-xl font-semibold text-slate-900 dark:text-slate-100">{meta.label}</h2>
      <p className="mt-1 text-sm text-slate-600 dark:text-slate-400">{meta.description}</p>

      <form className="mt-6 space-y-4" onSubmit={handleSubmit}>
        {renderFields(draft, setDraft)}

        {error ? (
          <p
            data-testid="credentials-form-error"
            className="text-sm text-red-700 dark:text-red-300"
          >
            {error}
          </p>
        ) : null}

        <div className="flex items-center justify-between pt-2">
          <button
            type="button"
            onClick={onBack}
            data-testid="credentials-form-back"
            className="text-sm text-slate-600 hover:text-slate-900 dark:text-slate-400 dark:hover:text-slate-100"
          >
            ← Back
          </button>
          <button
            type="submit"
            data-testid="credentials-form-continue"
            className="rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white shadow-sm hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-blue-500"
          >
            Continue
          </button>
        </div>
      </form>
    </section>
  );
}

// ---------------------------------------------------------------------------
// Per-kind field rendering
// ---------------------------------------------------------------------------

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
            helper={<span>Copy this from SigNoz Cloud → Settings → Ingestion Settings.</span>}
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
            <legend className="text-sm font-medium text-slate-900 dark:text-slate-100">
              Protocol
            </legend>
            <div className="mt-2 flex gap-4">
              {(['http', 'grpc'] as const).map((p) => (
                <label key={p} className="flex items-center gap-2 text-sm">
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

// ---------------------------------------------------------------------------
// Field primitives
// ---------------------------------------------------------------------------

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
      <span className="text-sm font-medium text-slate-900 dark:text-slate-100">{label}</span>
      <input
        type="text"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        data-testid={testId}
        className="mt-1 w-full rounded-md border border-slate-300 bg-white px-3 py-2 text-sm shadow-sm focus:border-blue-500 focus:outline-none focus:ring-1 focus:ring-blue-500 dark:border-slate-700 dark:bg-slate-900"
      />
      {helper ? (
        <span className="mt-1 block text-xs text-slate-500 dark:text-slate-400">{helper}</span>
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
      <span className="text-sm font-medium text-slate-900 dark:text-slate-100">{label}</span>
      <div className="mt-1 flex items-center gap-2">
        <input
          type={reveal ? 'text' : 'password'}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          data-testid={testId}
          className="w-full rounded-md border border-slate-300 bg-white px-3 py-2 text-sm shadow-sm focus:border-blue-500 focus:outline-none focus:ring-1 focus:ring-blue-500 dark:border-slate-700 dark:bg-slate-900"
        />
        <button
          type="button"
          onClick={() => setReveal((r) => !r)}
          data-testid={`${testId}-reveal`}
          className="rounded-md border border-slate-300 px-2 py-1 text-xs text-slate-700 hover:bg-slate-100 dark:border-slate-700 dark:text-slate-300 dark:hover:bg-slate-800"
          aria-label={reveal ? 'Hide value' : 'Show value'}
        >
          {reveal ? 'Hide' : 'Show'}
        </button>
      </div>
      {helper ? (
        <span className="mt-1 block text-xs text-slate-500 dark:text-slate-400">{helper}</span>
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
    // Pick a unique placeholder name so editing immediately works.
    let i = 1;
    let name = 'header';
    while (Object.prototype.hasOwnProperty.call(headers, name)) {
      i += 1;
      name = `header${i}`;
    }
    onChange({ ...headers, [name]: '' });
  };

  return (
    <fieldset>
      <legend className="text-sm font-medium text-slate-900 dark:text-slate-100">
        Auth headers
      </legend>
      <p className="mt-1 text-xs text-slate-500 dark:text-slate-400">
        Each header value is stored in your OS keychain, never in plain text on disk.
      </p>

      <ul className="mt-2 space-y-2" data-testid="otlp-generic-headers">
        {entries.length === 0 ? (
          <li className="text-xs italic text-slate-500 dark:text-slate-400">No headers yet.</li>
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
                className="flex-1 rounded-md border border-slate-300 bg-white px-3 py-1 text-sm shadow-sm focus:border-blue-500 focus:outline-none focus:ring-1 focus:ring-blue-500 dark:border-slate-700 dark:bg-slate-900"
              />
              <input
                type="password"
                value={value}
                onChange={(e) => update(name, name, e.target.value)}
                placeholder="value"
                aria-label="Header value"
                data-testid={`otlp-generic-header-value-${name}`}
                className="flex-1 rounded-md border border-slate-300 bg-white px-3 py-1 text-sm shadow-sm focus:border-blue-500 focus:outline-none focus:ring-1 focus:ring-blue-500 dark:border-slate-700 dark:bg-slate-900"
              />
              <button
                type="button"
                onClick={() => remove(name)}
                data-testid={`otlp-generic-header-remove-${name}`}
                aria-label={`Remove ${name}`}
                className="rounded-md border border-slate-300 px-2 py-1 text-xs text-slate-700 hover:bg-slate-100 dark:border-slate-700 dark:text-slate-300 dark:hover:bg-slate-800"
              >
                Remove
              </button>
            </li>
          ))
        )}
      </ul>

      <button
        type="button"
        onClick={addRow}
        data-testid="otlp-generic-header-add"
        className="mt-2 rounded-md border border-dashed border-slate-400 px-3 py-1 text-xs text-slate-600 hover:border-blue-500 hover:text-blue-700 dark:border-slate-600 dark:text-slate-400 dark:hover:border-blue-400 dark:hover:text-blue-300"
      >
        + Add header
      </button>
    </fieldset>
  );
}
