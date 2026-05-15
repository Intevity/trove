import { BookOpen, Lock, Plus, Trash2, AlertTriangle } from 'lucide-react';
import { useState, type JSX } from 'react';

import { type TroveMetricDefinition, type TroveMetricKind } from '@trove/shared';

import { Button, Card, CardHeader, CardTitle, Pill } from '../../ui/index.js';
import { validateCatalog, type CatalogIssue } from './validate.js';

interface Props {
  catalog: TroveMetricDefinition[];
  /** Whether the catalog has unsaved edits relative to the persisted state. */
  dirty: boolean;
  onChange: (next: TroveMetricDefinition[]) => void;
}

/** Editable metric catalog. Builtins (5 ships-with Tier A metrics) are
 *  locked: rename, kind, requiredAttributes, and delete are all
 *  disabled. The user can edit the description only. Custom rows are
 *  fully editable; new rows are added via [+ Add metric] and seeded
 *  with reasonable defaults. */
export function MetricsCatalog({ catalog, dirty, onChange }: Props): JSX.Element {
  const issues = validateCatalog(catalog);
  const [showAddForm, setShowAddForm] = useState(false);

  const updateAt = (index: number, patch: Partial<TroveMetricDefinition>) => {
    onChange(catalog.map((m, i) => (i === index ? { ...m, ...patch } : m)));
  };
  const removeAt = (index: number) => {
    onChange(catalog.filter((_, i) => i !== index));
  };
  const addNew = (def: TroveMetricDefinition) => {
    onChange([...catalog, def]);
    setShowAddForm(false);
  };

  return (
    <Card>
      <CardHeader className="flex-col items-stretch gap-0">
        <div className="flex flex-nowrap items-center justify-between gap-x-6">
          <CardTitle className="flex items-center gap-1.5 whitespace-nowrap">
            <BookOpen size={14} strokeWidth={2.4} className="text-brand" aria-hidden="true" />
            Metrics catalog
          </CardTitle>
          <div className="flex flex-nowrap items-center gap-2">
            {dirty ? <Pill tone="amber">Modified</Pill> : null}
            <Button
              size="sm"
              variant="secondary"
              onClick={() => setShowAddForm((v) => !v)}
              testid="catalog-add-toggle"
            >
              <Plus className="h-3 w-3" /> Add metric
            </Button>
          </div>
        </div>
        <p className="mt-2 text-[12px] text-fg-tertiary dark:text-fg-tertiary-dark">
          The five builtin Tier A metrics ship locked. Custom metrics extend the schema; they flow
          through the collector for native-OTel harnesses but require dashboards on the downstream
          backend to interpret them.
        </p>
      </CardHeader>

      {showAddForm ? (
        <AddMetricForm
          existingIds={new Set(catalog.map((m) => m.id))}
          onCancel={() => setShowAddForm(false)}
          onAdd={addNew}
        />
      ) : null}

      <ul className="mt-2 flex flex-col gap-1.5">
        {catalog.map((metric, idx) => (
          <li key={`${metric.id}-${idx}`}>
            <MetricRow
              metric={metric}
              issues={issues.filter((i) => i.metricIndex === idx)}
              onChange={(patch) => updateAt(idx, patch)}
              onDelete={() => removeAt(idx)}
            />
          </li>
        ))}
      </ul>
    </Card>
  );
}

function MetricRow({
  metric,
  issues,
  onChange,
  onDelete,
}: {
  metric: TroveMetricDefinition;
  issues: CatalogIssue[];
  onChange: (patch: Partial<TroveMetricDefinition>) => void;
  onDelete: () => void;
}): JSX.Element {
  const [expanded, setExpanded] = useState(false);
  const hasErrors = issues.some((i) => i.severity === 'error');

  return (
    <div
      className={`rounded-md border ${
        hasErrors
          ? 'border-ios-red/40 bg-ios-red/[0.04]'
          : 'border-black/[0.06] dark:border-white/[0.08]'
      } px-2.5 py-1.5`}
    >
      <button
        type="button"
        onClick={() => setExpanded((v) => !v)}
        className="flex w-full items-center justify-between text-left"
      >
        <div className="flex items-center gap-2 text-[12px]">
          {metric.builtin ? (
            <Lock className="h-3 w-3 text-fg-tertiary" aria-label="Builtin (locked)" />
          ) : (
            <Pill tone="brand" size="xs">
              custom
            </Pill>
          )}
          <code className="font-mono text-fg-primary dark:text-fg-primary-dark">{metric.name}</code>
          <span className="text-fg-tertiary dark:text-fg-tertiary-dark">·</span>
          <span className="text-fg-tertiary dark:text-fg-tertiary-dark">{metric.kind}</span>
          {metric.requiredAttributes.length > 0 ? (
            <>
              <span className="text-fg-tertiary dark:text-fg-tertiary-dark">·</span>
              <span className="font-mono text-[11px] text-fg-tertiary dark:text-fg-tertiary-dark">
                requires {metric.requiredAttributes.join(', ')}
              </span>
            </>
          ) : null}
        </div>
        <div className="flex items-center gap-1.5">
          {hasErrors ? <AlertTriangle className="h-3.5 w-3.5 text-ios-red" /> : null}
          <span className="text-[11px] text-fg-tertiary dark:text-fg-tertiary-dark">
            {expanded ? 'collapse' : 'edit'}
          </span>
        </div>
      </button>

      {expanded ? (
        <div className="mt-2 flex flex-col gap-2 border-t border-black/[0.06] pt-2 dark:border-white/[0.08]">
          <LabeledInput
            label="ID"
            value={metric.id}
            disabled={metric.builtin}
            onChange={(v) => onChange({ id: v })}
            mono
          />
          <LabeledInput
            label="Wire name"
            value={metric.name}
            disabled={metric.builtin}
            onChange={(v) => onChange({ name: v })}
            mono
          />
          <LabeledSelect
            label="Kind"
            value={metric.kind}
            disabled={metric.builtin}
            options={['counter', 'gauge', 'histogram'] as const}
            onChange={(v) => onChange({ kind: v as TroveMetricKind })}
          />
          <LabeledInput
            label="Description"
            value={metric.description}
            onChange={(v) => onChange({ description: v })}
          />
          <AttributeListField
            label="Required attributes"
            values={metric.requiredAttributes}
            disabled={metric.builtin}
            onChange={(values) => onChange({ requiredAttributes: values })}
          />
          {issues.length > 0 ? (
            <ul className="flex flex-col gap-0.5 text-[11px] text-ios-red">
              {issues.map((i, k) => (
                <li key={k}>· {i.message}</li>
              ))}
            </ul>
          ) : null}
          {!metric.builtin ? (
            <div className="flex justify-end">
              <Button size="sm" variant="destructive" onClick={onDelete}>
                <Trash2 className="h-3 w-3" /> Delete
              </Button>
            </div>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

function AddMetricForm({
  existingIds,
  onCancel,
  onAdd,
}: {
  existingIds: Set<string>;
  onCancel: () => void;
  onAdd: (def: TroveMetricDefinition) => void;
}): JSX.Element {
  const [id, setId] = useState('');
  const [name, setName] = useState('');
  const [kind, setKind] = useState<TroveMetricKind>('counter');
  const [requiredAttributes, setRequiredAttributes] = useState<string[]>([]);
  const [description, setDescription] = useState('');

  const idOk = id.trim().length > 0 && /^[a-z][a-z0-9._-]*$/.test(id) && !existingIds.has(id);
  const nameOk = name.trim().length > 0;

  return (
    <div className="mt-3 rounded-md border border-brand/30 bg-brand/[0.04] p-3">
      <p className="mb-2 text-[12px] font-medium text-fg-primary dark:text-fg-primary-dark">
        New metric
      </p>
      <div className="mb-1.5 flex items-start gap-2 rounded-md bg-amber-500/[0.08] p-2 text-[11px] text-fg-secondary dark:text-fg-secondary-dark">
        <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0 text-amber-600" />
        <span>
          Custom metrics don&apos;t carry across teams; anyone consuming Trove data needs the same
          catalog to interpret them. Prefer the five builtins when possible.
        </span>
      </div>
      <div className="flex flex-col gap-1.5">
        <LabeledInput label="ID" value={id} onChange={setId} mono placeholder="e.g. tool_calls" />
        <LabeledInput
          label="Wire name"
          value={name}
          onChange={setName}
          mono
          placeholder="e.g. my.team.tool_calls"
        />
        <LabeledSelect
          label="Kind"
          value={kind}
          options={['counter', 'gauge', 'histogram'] as const}
          onChange={(v) => setKind(v as TroveMetricKind)}
        />
        <LabeledInput
          label="Description"
          value={description}
          onChange={setDescription}
          placeholder="optional"
        />
        <AttributeListField
          label="Required attributes"
          values={requiredAttributes}
          onChange={setRequiredAttributes}
        />
      </div>
      <div className="mt-2 flex justify-end gap-2">
        <Button size="sm" variant="ghost" onClick={onCancel}>
          Cancel
        </Button>
        <Button
          size="sm"
          variant="primary"
          disabled={!idOk || !nameOk}
          onClick={() => onAdd({ id, name, kind, description, requiredAttributes, builtin: false })}
        >
          Add
        </Button>
      </div>
    </div>
  );
}

function LabeledInput({
  label,
  value,
  onChange,
  disabled,
  mono,
  placeholder,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  disabled?: boolean;
  mono?: boolean;
  placeholder?: string;
}): JSX.Element {
  return (
    <label className="flex items-center gap-2 text-[12px]">
      <span className="w-32 text-fg-tertiary dark:text-fg-tertiary-dark">{label}</span>
      <input
        className={`flex-1 rounded-md border border-black/[0.08] bg-white px-2 py-1 text-[12px] outline-none focus:border-brand focus:ring-1 focus:ring-brand disabled:opacity-60 dark:border-white/[0.12] dark:bg-canvas-dark ${
          mono ? 'font-mono' : ''
        }`}
        value={value}
        disabled={disabled}
        placeholder={placeholder}
        onChange={(e) => onChange(e.target.value)}
      />
    </label>
  );
}

function LabeledSelect<T extends string>({
  label,
  value,
  options,
  onChange,
  disabled,
}: {
  label: string;
  value: T;
  options: readonly T[];
  onChange: (v: T) => void;
  disabled?: boolean;
}): JSX.Element {
  return (
    <label className="flex items-center gap-2 text-[12px]">
      <span className="w-32 text-fg-tertiary dark:text-fg-tertiary-dark">{label}</span>
      <select
        className="flex-1 rounded-md border border-black/[0.08] bg-white px-2 py-1 text-[12px] outline-none focus:border-brand focus:ring-1 focus:ring-brand disabled:opacity-60 dark:border-white/[0.12] dark:bg-canvas-dark"
        value={value}
        disabled={disabled}
        onChange={(e) => onChange(e.target.value as T)}
      >
        {options.map((o) => (
          <option key={o} value={o}>
            {o}
          </option>
        ))}
      </select>
    </label>
  );
}

function AttributeListField({
  label,
  values,
  onChange,
  disabled,
}: {
  label: string;
  values: string[];
  onChange: (next: string[]) => void;
  disabled?: boolean;
}): JSX.Element {
  const [draft, setDraft] = useState('');
  return (
    <div className="flex items-start gap-2 text-[12px]">
      <span className="mt-1 w-32 text-fg-tertiary dark:text-fg-tertiary-dark">{label}</span>
      <div className="flex flex-1 flex-wrap items-center gap-1">
        {values.map((v, i) => (
          <span
            key={`${v}-${i}`}
            className="inline-flex items-center gap-1 rounded-full bg-black/[0.06] px-2 py-0.5 font-mono text-[11px] dark:bg-white/[0.08]"
          >
            {v}
            {!disabled ? (
              <button
                type="button"
                className="ml-0.5 text-fg-tertiary hover:text-ios-red"
                onClick={() => onChange(values.filter((_, k) => k !== i))}
                aria-label={`Remove ${v}`}
              >
                ×
              </button>
            ) : null}
          </span>
        ))}
        {!disabled ? (
          <input
            className="min-w-[120px] flex-1 rounded-md border border-black/[0.08] bg-white px-1.5 py-0.5 font-mono text-[11px] outline-none focus:border-brand focus:ring-1 focus:ring-brand dark:border-white/[0.12] dark:bg-canvas-dark"
            placeholder="add attribute…"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && draft.trim()) {
                e.preventDefault();
                onChange([...values, draft.trim()]);
                setDraft('');
              }
            }}
          />
        ) : null}
      </div>
    </div>
  );
}
