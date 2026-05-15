import {
  DndContext,
  KeyboardSensor,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
  type DragEndEvent,
  type DraggableAttributes,
} from '@dnd-kit/core';
import {
  SortableContext,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from '@dnd-kit/sortable';
import { CSS } from '@dnd-kit/utilities';
import {
  AlertTriangle,
  Copy,
  Eye,
  GitCompareArrows,
  GripVertical,
  Plus,
  Trash2,
} from 'lucide-react';
import { useState, type JSX } from 'react';

import {
  HarnessMapping as HarnessMappingSchema,
  type HarnessMapping,
  type MappingSource,
  type TroveMetricDefinition,
} from '@trove/shared';

import { HarnessLogo } from '../../../lib/logos.js';
import { Button, Card, CardHeader, CardTitle, Pill, StatusDot } from '../../ui/index.js';
import {
  blankHookRule,
  blankSynthesisRule,
  hasBlockingErrors,
  validateHarnessMapping,
  type RuleIssue,
} from './validate.js';

interface Props {
  /** Persisted form — what apply will overwrite. */
  persisted: HarnessMapping;
  /** Draft form — what the user is editing. May === persisted when clean. */
  draft: HarnessMapping;
  /** Whether `draft` differs from `persisted`. */
  dirty: boolean;
  /** Catalog from the parent draft (lets newly-added custom metrics flow
   *  into this editor's target dropdowns immediately). */
  catalog: TroveMetricDefinition[];
  busy: boolean;
  onDraftChange: (next: HarnessMapping) => void;
  onApply: () => void;
  onDiscard: () => void;
  onPreviewRule: (sourceIndex: number) => void;
  onShowDiff: () => void;
}

type ViewMode = 'visual' | 'json';

export function HarnessMappingEditor({
  persisted,
  draft,
  dirty,
  catalog,
  busy,
  onDraftChange,
  onApply,
  onDiscard,
  onPreviewRule,
  onShowDiff,
}: Props): JSX.Element {
  const [mode, setMode] = useState<ViewMode>('visual');
  const issues = validateHarnessMapping(draft, catalog);
  const blocked = hasBlockingErrors(issues);

  const updateSourceAt = (index: number, patch: MappingSource | null) => {
    const sources =
      patch === null
        ? draft.sources.filter((_, i) => i !== index)
        : draft.sources.map((s, i) => (i === index ? patch : s));
    onDraftChange({ ...draft, sources });
  };
  const insertSource = (kind: 'synthesize-from-native' | 'hook-rule') => {
    const blank = kind === 'hook-rule' ? blankHookRule(catalog) : blankSynthesisRule(catalog);
    onDraftChange({ ...draft, sources: [...draft.sources, blank] });
  };
  const reorderSources = (next: MappingSource[]) => {
    onDraftChange({ ...draft, sources: next });
  };
  const duplicateAt = (index: number) => {
    const orig = draft.sources[index];
    if (!orig) return;
    const copy = structuredClone(orig);
    const sources = [...draft.sources.slice(0, index + 1), copy, ...draft.sources.slice(index + 1)];
    onDraftChange({ ...draft, sources });
  };

  return (
    <Card>
      <CardHeader>
        <div className="flex flex-nowrap items-center justify-between gap-x-6">
          <CardTitle className="flex items-center gap-2 whitespace-nowrap">
            <HarnessLogo id={persisted.harnessId} size={20} />
            <span className="font-mono">{persisted.harnessId}</span>
          </CardTitle>
          <div className="flex flex-nowrap items-center gap-2">
            {dirty ? <Pill tone="amber">Modified</Pill> : null}
            <ModeToggle value={mode} onChange={setMode} />
          </div>
        </div>
        <div className="mt-1 flex items-center gap-3 text-[12px] text-fg-tertiary dark:text-fg-tertiary-dark">
          <label className="inline-flex items-center gap-1.5">
            <input
              type="checkbox"
              checked={draft.enabled}
              onChange={(e) => onDraftChange({ ...draft, enabled: e.target.checked })}
              className="h-3 w-3"
            />
            <StatusDot status={draft.enabled ? 'green' : 'red'} size="sm" />
            {draft.enabled ? 'Enabled' : 'Disabled'}
          </label>
          <span>·</span>
          <span>
            {draft.sources.filter((s) => s.kind === 'synthesize-from-native').length} synthesis,{' '}
            {draft.sources.filter((s) => s.kind === 'hook-rule').length} hook
          </span>
        </div>
      </CardHeader>

      {mode === 'visual' ? (
        <VisualBody
          draft={draft}
          catalog={catalog}
          issues={issues}
          onUpdateAt={updateSourceAt}
          onInsert={insertSource}
          onReorder={reorderSources}
          onDuplicate={duplicateAt}
          onPreviewRule={onPreviewRule}
        />
      ) : (
        <JsonBody draft={draft} onDraftChange={onDraftChange} />
      )}

      {dirty ? (
        <div className="mt-3 flex items-center justify-between gap-2 border-t border-black/[0.06] pt-2 dark:border-white/[0.08]">
          <div className="text-[11px] text-fg-tertiary dark:text-fg-tertiary-dark">
            {blocked
              ? `${issues.filter((i) => i.severity === 'error').length} error(s) — fix before applying`
              : 'Draft ready to apply'}
          </div>
          <div className="flex items-center gap-2">
            <Button size="sm" variant="ghost" onClick={onShowDiff}>
              <GitCompareArrows className="h-3 w-3" /> View diff
            </Button>
            <Button size="sm" variant="secondary" onClick={onDiscard} disabled={busy}>
              Discard
            </Button>
            <Button
              size="sm"
              variant="primary"
              onClick={onApply}
              disabled={busy || blocked}
              loading={busy}
            >
              Apply
            </Button>
          </div>
        </div>
      ) : null}
    </Card>
  );
}

function ModeToggle({
  value,
  onChange,
}: {
  value: ViewMode;
  onChange: (m: ViewMode) => void;
}): JSX.Element {
  return (
    <div className="inline-flex rounded-md border border-black/[0.08] bg-white/40 p-0.5 text-[11px] dark:border-white/[0.12] dark:bg-canvas-dark/40">
      {(['visual', 'json'] as const).map((m) => (
        <button
          key={m}
          type="button"
          onClick={() => onChange(m)}
          className={`rounded px-2 py-0.5 ${
            value === m ? 'bg-brand text-white' : 'text-fg-secondary dark:text-fg-secondary-dark'
          }`}
        >
          {m === 'visual' ? 'Visual' : 'JSON'}
        </button>
      ))}
    </div>
  );
}

function VisualBody({
  draft,
  catalog,
  issues,
  onUpdateAt,
  onInsert,
  onReorder,
  onDuplicate,
  onPreviewRule,
}: {
  draft: HarnessMapping;
  catalog: TroveMetricDefinition[];
  issues: RuleIssue[];
  onUpdateAt: (index: number, patch: MappingSource | null) => void;
  onInsert: (kind: 'synthesize-from-native' | 'hook-rule') => void;
  onReorder: (next: MappingSource[]) => void;
  onDuplicate: (index: number) => void;
  onPreviewRule: (index: number) => void;
}): JSX.Element {
  return (
    <div className="mt-2 flex flex-col gap-4">
      <RuleSection
        title="Synthesis rules"
        emptyHint="No synthesis rules. Native-OTel harnesses use these to emit Tier A metrics."
        addLabel="+ Add synthesis rule"
        onAdd={() => onInsert('synthesize-from-native')}
        sources={draft.sources}
        filterKind="synthesize-from-native"
        catalog={catalog}
        issues={issues}
        onUpdateAt={onUpdateAt}
        onReorder={onReorder}
        onDuplicate={onDuplicate}
        onPreviewRule={onPreviewRule}
      />
      <RuleSection
        title="Hook rules"
        emptyHint="No hook rules. Hook/watcher harnesses use these to translate raw events into Tier A."
        addLabel="+ Add hook rule"
        onAdd={() => onInsert('hook-rule')}
        sources={draft.sources}
        filterKind="hook-rule"
        catalog={catalog}
        issues={issues}
        onUpdateAt={onUpdateAt}
        onReorder={onReorder}
        onDuplicate={onDuplicate}
        onPreviewRule={onPreviewRule}
      />
    </div>
  );
}

function RuleSection({
  title,
  emptyHint,
  addLabel,
  onAdd,
  sources,
  filterKind,
  catalog,
  issues,
  onUpdateAt,
  onReorder,
  onDuplicate,
  onPreviewRule,
}: {
  title: string;
  emptyHint: string;
  addLabel: string;
  onAdd: () => void;
  sources: MappingSource[];
  filterKind: MappingSource['kind'];
  catalog: TroveMetricDefinition[];
  issues: RuleIssue[];
  onUpdateAt: (index: number, patch: MappingSource | null) => void;
  onReorder: (next: MappingSource[]) => void;
  onDuplicate: (index: number) => void;
  onPreviewRule: (index: number) => void;
}): JSX.Element {
  // Map each filtered row back to its index in the full `sources` array
  // — that's what onUpdateAt and the issue lookups key on. The dnd-kit
  // SortableContext keys on the same indices (as strings), so reorders
  // within the filtered section translate cleanly back to a full-array
  // swap.
  const matchingIndices = sources
    .map((s, i) => ({ s, i }))
    .filter(({ s }) => s.kind === filterKind);

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 4 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );

  const handleDragEnd = (event: DragEndEvent) => {
    const { active, over } = event;
    if (!over || active.id === over.id) return;
    const activeIdx = Number(active.id);
    const overIdx = Number(over.id);
    if (!Number.isFinite(activeIdx) || !Number.isFinite(overIdx)) return;
    // Build the reordered array of *full-array indices* among the
    // filtered subset, then write back into the full sources array.
    const ids = matchingIndices.map(({ i }) => i);
    const fromDisplay = ids.indexOf(activeIdx);
    const toDisplay = ids.indexOf(overIdx);
    if (fromDisplay < 0 || toDisplay < 0) return;
    const reorderedIds = ids.slice();
    const [moved] = reorderedIds.splice(fromDisplay, 1);
    if (moved === undefined) return;
    reorderedIds.splice(toDisplay, 0, moved);
    // Re-stitch into the full `sources` array: walk the original array
    // and, at each position that was in the filtered set, take the next
    // id from `reorderedIds`.
    const idQueue = reorderedIds.slice();
    const next: MappingSource[] = sources.map((s) => {
      if (s.kind !== filterKind) return s;
      const nextId = idQueue.shift();
      // Defensive: should always be defined.
      return nextId !== undefined ? sources[nextId]! : s;
    });
    onReorder(next);
  };

  return (
    <div>
      <div className="mb-1 flex items-center justify-between">
        <span className="text-[12px] font-medium text-fg-secondary dark:text-fg-secondary-dark">
          {title}
        </span>
        <Button size="sm" variant="ghost" onClick={onAdd}>
          <Plus className="h-3 w-3" /> {addLabel.replace('+ ', '')}
        </Button>
      </div>
      {matchingIndices.length === 0 ? (
        <p className="rounded-md border border-dashed border-black/[0.08] px-2.5 py-2 text-[11px] text-fg-tertiary dark:border-white/[0.12] dark:text-fg-tertiary-dark">
          {emptyHint}
        </p>
      ) : (
        <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={handleDragEnd}>
          <SortableContext
            items={matchingIndices.map(({ i }) => String(i))}
            strategy={verticalListSortingStrategy}
          >
            <ul className="flex flex-col gap-1">
              {matchingIndices.map(({ s, i }) => (
                <SortableRuleRow
                  key={i}
                  id={String(i)}
                  source={s}
                  catalog={catalog}
                  issues={issues.filter((iss) => iss.ruleIndex === i)}
                  onChange={(patch) => onUpdateAt(i, patch)}
                  onDelete={() => onUpdateAt(i, null)}
                  onDuplicate={() => onDuplicate(i)}
                  onPreview={() => onPreviewRule(i)}
                />
              ))}
            </ul>
          </SortableContext>
        </DndContext>
      )}
    </div>
  );
}

function SortableRuleRow(props: {
  id: string;
  source: MappingSource;
  catalog: TroveMetricDefinition[];
  issues: RuleIssue[];
  onChange: (next: MappingSource) => void;
  onDelete: () => void;
  onDuplicate: () => void;
  onPreview: () => void;
}): JSX.Element {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: props.id,
  });
  const style = {
    transform: CSS.Transform.toString(transform),
    transition,
    opacity: isDragging ? 0.7 : 1,
  };
  return (
    <div ref={setNodeRef} style={style}>
      <RuleRow {...props} dragHandleProps={{ attributes, listeners }} />
    </div>
  );
}

interface DragHandleProps {
  attributes: DraggableAttributes;
  listeners: ReturnType<typeof useSortable>['listeners'];
}

function RuleRow({
  source,
  catalog,
  issues,
  onChange,
  onDelete,
  onDuplicate,
  onPreview,
  dragHandleProps,
}: {
  source: MappingSource;
  catalog: TroveMetricDefinition[];
  issues: RuleIssue[];
  onChange: (next: MappingSource) => void;
  onDelete: () => void;
  onDuplicate: () => void;
  onPreview: () => void;
  dragHandleProps?: DragHandleProps | undefined;
}): JSX.Element {
  const [expanded, setExpanded] = useState(false);
  const errorIssues = issues.filter((i) => i.severity === 'error');
  const warningIssues = issues.filter((i) => i.severity === 'warning');
  const hasErrors = errorIssues.length > 0;
  const hasWarnings = warningIssues.length > 0;
  // Surface the first issue inline on the collapsed row so users see
  // *why* the row is flagged without needing to expand it. Errors take
  // precedence over warnings. Expand the row to see the full list.
  const summaryIssue = errorIssues[0] ?? warningIssues[0];

  return (
    <li
      className={`rounded-md border ${
        hasErrors
          ? 'border-ios-red/40 bg-ios-red/[0.04]'
          : hasWarnings
            ? 'border-amber-500/40 bg-amber-500/[0.04]'
            : 'border-black/[0.06] dark:border-white/[0.08]'
      } px-2 py-1.5`}
    >
      <div className="flex items-center gap-2">
        {dragHandleProps ? (
          <button
            type="button"
            className="cursor-grab touch-none rounded p-1 text-fg-tertiary hover:bg-black/[0.06] hover:text-fg-primary active:cursor-grabbing dark:hover:bg-white/[0.08] dark:hover:text-fg-primary-dark"
            title="Drag to reorder (or focus + space to lift with keyboard)"
            aria-label="Drag to reorder"
            {...dragHandleProps.attributes}
            {...(dragHandleProps.listeners ?? {})}
          >
            <GripVertical className="h-3.5 w-3.5" />
          </button>
        ) : null}
        <button
          type="button"
          onClick={() => setExpanded((v) => !v)}
          className="min-w-0 flex-1 text-left"
          aria-expanded={expanded}
        >
          <RuleSummary source={source} catalog={catalog} />
        </button>
        <div className="flex shrink-0 items-center gap-0.5 opacity-70 hover:opacity-100">
          {hasErrors ? (
            <AlertTriangle className="h-3.5 w-3.5 text-ios-red" aria-label="Has errors" />
          ) : hasWarnings ? (
            <AlertTriangle className="h-3.5 w-3.5 text-amber-600" aria-label="Has warnings" />
          ) : null}
          <IconButton title="Preview" onClick={onPreview}>
            <Eye className="h-3 w-3" />
          </IconButton>
          <IconButton title="Duplicate" onClick={onDuplicate}>
            <Copy className="h-3 w-3" />
          </IconButton>
          <IconButton title="Delete" onClick={onDelete}>
            <Trash2 className="h-3 w-3" />
          </IconButton>
        </div>
      </div>
      {!expanded && summaryIssue ? (
        <p
          className={`mt-1 text-[11px] ${
            summaryIssue.severity === 'error' ? 'text-ios-red' : 'text-amber-600'
          }`}
          title={issues.length > 1 ? `${issues.length - 1} more — expand to see all` : undefined}
        >
          {summaryIssue.message}
          {issues.length > 1 ? (
            <span className="ml-1 text-fg-tertiary dark:text-fg-tertiary-dark">
              (+{issues.length - 1} more)
            </span>
          ) : null}
        </p>
      ) : null}
      {expanded ? (
        <div className="mt-2 border-t border-black/[0.06] pt-2 dark:border-white/[0.08]">
          {source.kind === 'synthesize-from-native' ? (
            <SynthesisRuleForm source={source} catalog={catalog} onChange={onChange} />
          ) : (
            <HookRuleForm source={source} catalog={catalog} onChange={onChange} />
          )}
          {issues.length > 0 ? (
            <ul className="mt-1.5 flex flex-col gap-0.5 text-[11px]">
              {issues.map((i, k) => (
                <li key={k} className={i.severity === 'error' ? 'text-ios-red' : 'text-amber-600'}>
                  · {i.message}
                </li>
              ))}
            </ul>
          ) : null}
        </div>
      ) : null}
    </li>
  );
}

function IconButton({
  title,
  onClick,
  children,
}: {
  title: string;
  onClick: () => void;
  children: JSX.Element;
}): JSX.Element {
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      aria-label={title}
      className="rounded p-1 hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
    >
      {children}
    </button>
  );
}

function RuleSummary({
  source,
  catalog,
}: {
  source: MappingSource;
  catalog: TroveMetricDefinition[];
}): JSX.Element {
  if (source.kind === 'synthesize-from-native') {
    const target = catalog.find((m) => m.id === source.targetMetric);
    return (
      <span className="font-mono text-[11px]">
        <code className="rounded bg-black/[0.06] px-1 py-0.5 dark:bg-white/[0.08]">
          {source.nativeMetric || '(empty)'}
        </code>{' '}
        →{' '}
        <code className="rounded bg-ios-green/[0.14] px-1 py-0.5 text-ios-green">
          {target?.name ?? source.targetMetric}
        </code>
        {Object.keys(source.attributeMap).length > 0 ? (
          <span className="ml-1 text-fg-tertiary dark:text-fg-tertiary-dark">
            renames{' '}
            {Object.entries(source.attributeMap)
              .map(([k, v]) => `${k}→${v}`)
              .join(', ')}
          </span>
        ) : null}
      </span>
    );
  }
  // hook-rule
  const target = source.emit ? catalog.find((m) => m.id === source.emit?.metric) : null;
  return (
    <span className="font-mono text-[11px]">
      <code className="rounded bg-black/[0.06] px-1 py-0.5 dark:bg-white/[0.08]">
        {source.when || '(empty)'}
      </code>{' '}
      →{' '}
      {source.emit === null ? (
        <span className="italic text-fg-tertiary dark:text-fg-tertiary-dark">no emission</span>
      ) : (
        <code className="rounded bg-ios-green/[0.14] px-1 py-0.5 text-ios-green">
          {target?.name ?? source.emit.metric}
        </code>
      )}
    </span>
  );
}

function SynthesisRuleForm({
  source,
  catalog,
  onChange,
}: {
  source: Extract<MappingSource, { kind: 'synthesize-from-native' }>;
  catalog: TroveMetricDefinition[];
  onChange: (next: MappingSource) => void;
}): JSX.Element {
  return (
    <div className="flex flex-col gap-1.5 text-[12px]">
      <Field label="Native metric">
        <input
          className="flex-1 rounded-md border border-black/[0.08] bg-white px-2 py-1 font-mono text-[12px] outline-none focus:border-brand focus:ring-1 focus:ring-brand dark:border-white/[0.12] dark:bg-canvas-dark"
          value={source.nativeMetric}
          onChange={(e) => onChange({ ...source, nativeMetric: e.target.value })}
          placeholder="e.g. claude_code.token.usage"
        />
      </Field>
      <Field label="Target metric">
        <MetricPicker
          value={source.targetMetric}
          catalog={catalog}
          onChange={(v) => onChange({ ...source, targetMetric: v })}
        />
      </Field>
      <Field label="Rename attrs">
        <KeyValueEditor
          values={source.attributeMap}
          onChange={(attributeMap) => onChange({ ...source, attributeMap })}
          keyPlaceholder="source key"
          valuePlaceholder="target key"
        />
      </Field>
      <Field label="Inject attrs">
        <KeyValueEditor
          values={source.injectAttributes}
          onChange={(injectAttributes) => onChange({ ...source, injectAttributes })}
          keyPlaceholder="key"
          valuePlaceholder="constant value"
        />
      </Field>
    </div>
  );
}

function HookRuleForm({
  source,
  catalog,
  onChange,
}: {
  source: Extract<MappingSource, { kind: 'hook-rule' }>;
  catalog: TroveMetricDefinition[];
  onChange: (next: MappingSource) => void;
}): JSX.Element {
  const suppress = source.emit === null;
  return (
    <div className="flex flex-col gap-1.5 text-[12px]">
      <Field label="Event name">
        <input
          className="flex-1 rounded-md border border-black/[0.08] bg-white px-2 py-1 font-mono text-[12px] outline-none focus:border-brand focus:ring-1 focus:ring-brand dark:border-white/[0.12] dark:bg-canvas-dark"
          value={source.when}
          onChange={(e) => onChange({ ...source, when: e.target.value })}
          placeholder="e.g. afterAgentResponse"
        />
      </Field>
      <label className="ml-32 inline-flex items-center gap-1.5 text-[12px]">
        <input
          type="checkbox"
          checked={suppress}
          onChange={(e) =>
            onChange({
              ...source,
              emit: e.target.checked
                ? null
                : { metric: catalog[0]?.id ?? 'events', attributes: {} },
            })
          }
        />
        Suppress emission (used to avoid double-counting against an after-event rule)
      </label>
      {!suppress && source.emit ? (
        <>
          <Field label="Target metric">
            <MetricPicker
              value={source.emit.metric}
              catalog={catalog}
              onChange={(v) => onChange({ ...source, emit: { ...source.emit!, metric: v } })}
            />
          </Field>
          <Field label="Attributes">
            <KeyValueEditor
              values={source.emit.attributes}
              onChange={(attributes) =>
                onChange({ ...source, emit: { ...source.emit!, attributes } })
              }
              keyPlaceholder="key (e.g. event.kind)"
              valuePlaceholder="value (e.g. chat.turn)"
            />
          </Field>
        </>
      ) : null}
    </div>
  );
}

function MetricPicker({
  value,
  catalog,
  onChange,
}: {
  value: string;
  catalog: TroveMetricDefinition[];
  onChange: (v: string) => void;
}): JSX.Element {
  return (
    <select
      className="flex-1 rounded-md border border-black/[0.08] bg-white px-2 py-1 text-[12px] outline-none focus:border-brand focus:ring-1 focus:ring-brand dark:border-white/[0.12] dark:bg-canvas-dark"
      value={value}
      onChange={(e) => onChange(e.target.value)}
    >
      {catalog.length === 0 ? <option value="">(no metrics defined)</option> : null}
      {catalog.map((m) => (
        <option key={m.id} value={m.id}>
          {m.name} {m.builtin ? '' : '(custom)'}
        </option>
      ))}
    </select>
  );
}

function KeyValueEditor({
  values,
  onChange,
  keyPlaceholder,
  valuePlaceholder,
}: {
  values: Record<string, string>;
  onChange: (next: Record<string, string>) => void;
  keyPlaceholder?: string;
  valuePlaceholder?: string;
}): JSX.Element {
  const [k, setK] = useState('');
  const [v, setV] = useState('');
  return (
    <div className="flex flex-1 flex-col gap-1">
      {Object.entries(values).map(([entryK, entryV]) => (
        <div key={entryK} className="flex items-center gap-1">
          <code className="flex-1 rounded bg-black/[0.04] px-1.5 py-0.5 font-mono text-[11px] dark:bg-white/[0.06]">
            {entryK}
          </code>
          <span className="text-fg-tertiary">→</span>
          <code className="flex-1 rounded bg-black/[0.04] px-1.5 py-0.5 font-mono text-[11px] dark:bg-white/[0.06]">
            {entryV}
          </code>
          <button
            type="button"
            onClick={() => {
              const next = { ...values };
              delete next[entryK];
              onChange(next);
            }}
            className="text-fg-tertiary hover:text-ios-red"
            aria-label={`Remove ${entryK}`}
          >
            ×
          </button>
        </div>
      ))}
      <div className="flex items-center gap-1">
        <input
          className="flex-1 rounded-md border border-black/[0.08] bg-white px-1.5 py-0.5 font-mono text-[11px] outline-none focus:border-brand focus:ring-1 focus:ring-brand dark:border-white/[0.12] dark:bg-canvas-dark"
          value={k}
          onChange={(e) => setK(e.target.value)}
          placeholder={keyPlaceholder}
        />
        <span className="text-fg-tertiary">→</span>
        <input
          className="flex-1 rounded-md border border-black/[0.08] bg-white px-1.5 py-0.5 font-mono text-[11px] outline-none focus:border-brand focus:ring-1 focus:ring-brand dark:border-white/[0.12] dark:bg-canvas-dark"
          value={v}
          onChange={(e) => setV(e.target.value)}
          placeholder={valuePlaceholder}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && k.trim() && v.trim()) {
              e.preventDefault();
              onChange({ ...values, [k.trim()]: v.trim() });
              setK('');
              setV('');
            }
          }}
        />
        <Button
          size="sm"
          variant="ghost"
          onClick={() => {
            if (k.trim() && v.trim()) {
              onChange({ ...values, [k.trim()]: v.trim() });
              setK('');
              setV('');
            }
          }}
          disabled={!k.trim() || !v.trim()}
        >
          add
        </Button>
      </div>
    </div>
  );
}

function Field({ label, children }: { label: string; children: JSX.Element }): JSX.Element {
  return (
    <label className="flex items-center gap-2">
      <span className="w-28 text-fg-tertiary dark:text-fg-tertiary-dark">{label}</span>
      {children}
    </label>
  );
}

function JsonBody({
  draft,
  onDraftChange,
}: {
  draft: HarnessMapping;
  onDraftChange: (next: HarnessMapping) => void;
}): JSX.Element {
  const [text, setText] = useState(() => JSON.stringify(draft, null, 2));
  const [parseError, setParseError] = useState<string | null>(null);

  return (
    <div className="mt-3">
      <p className="mb-1 text-[11px] text-fg-tertiary dark:text-fg-tertiary-dark">
        Paste-compatible JSON for this harness. Validated against the schema on blur.
      </p>
      <textarea
        className="h-72 w-full rounded-md border border-black/[0.08] bg-canvas px-2.5 py-2 font-mono text-[11px] leading-snug outline-none focus:border-brand focus:ring-1 focus:ring-brand dark:border-white/[0.12] dark:bg-canvas-dark"
        value={text}
        spellCheck={false}
        onChange={(e) => {
          setText(e.target.value);
          setParseError(null);
        }}
        onBlur={() => {
          try {
            const parsed = HarnessMappingSchema.parse(JSON.parse(text));
            if (parsed.harnessId !== draft.harnessId) {
              setParseError(
                `harnessId can't be changed from ${JSON.stringify(draft.harnessId)} in this card`,
              );
              return;
            }
            onDraftChange(parsed);
            setParseError(null);
          } catch (err) {
            setParseError(err instanceof Error ? err.message : String(err));
          }
        }}
      />
      {parseError ? <p className="mt-1 text-[11px] text-ios-red">{parseError}</p> : null}
    </div>
  );
}
