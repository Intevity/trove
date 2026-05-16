import {
  type HarnessMapping,
  type MappingSource,
  type MappingState,
  type TroveMetricDefinition,
} from '@trove/shared';

/** A client-side validation message. The Apply button gates on errors
 *  (`severity: 'error'`); warnings are advisory. */
export interface RuleIssue {
  ruleIndex: number;
  field?: 'when' | 'nativeMetric' | 'targetMetric' | 'attributeMap' | 'injectAttributes' | 'emit';
  severity: 'error' | 'warning';
  message: string;
}

/** Closed-enum domains for the well-known attribute keys. Mirrors the
 *  Rust `validate::allowed_values` table so Apply doesn't surface a
 *  surprise rejection after the inline UI claimed everything was fine. */
const CLOSED_ENUM_ATTRS: Record<string, string[]> = {
  'event.kind': [
    'chat.turn',
    'tool.call',
    'shell.exec',
    'file.edit',
    'session.start',
    'session.end',
  ],
  direction: ['input', 'output'],
  'cost.method': ['exact', 'estimated'],
  'error.kind': ['rate_limit', 'auth', 'tool_failure', 'network', 'policy', 'unknown'],
};

/** Validate every rule in `mapping`. Errors prevent Apply; warnings are
 *  surfaced inline but don't block. The catalog is needed to resolve
 *  target ids and check required-attribute coverage. */
export function validateHarnessMapping(
  mapping: HarnessMapping,
  catalog: TroveMetricDefinition[],
): RuleIssue[] {
  const issues: RuleIssue[] = [];
  const catalogById = new Map(catalog.map((m) => [m.id, m] as const));
  const seenWhenMetric = new Set<string>();

  mapping.sources.forEach((source, ruleIndex) => {
    if (source.kind === 'hook-rule') {
      if (!source.when.trim()) {
        issues.push({
          ruleIndex,
          field: 'when',
          severity: 'error',
          message: 'Event name is required',
        });
      }
      if (source.emit !== null) {
        const def = catalogById.get(source.emit.metric);
        if (!def) {
          issues.push({
            ruleIndex,
            field: 'emit',
            severity: 'error',
            message: `Target metric "${source.emit.metric}" is not in the catalog`,
          });
        } else {
          // Closed-enum domain checks on emit.attributes.
          for (const [k, v] of Object.entries(source.emit.attributes)) {
            const domain = CLOSED_ENUM_ATTRS[k];
            if (domain && !domain.includes(v)) {
              issues.push({
                ruleIndex,
                field: 'emit',
                severity: 'error',
                message: `Attribute ${k}=${JSON.stringify(v)} is not in the closed-enum domain {${domain.join(', ')}}`,
              });
            }
          }
          // Required-attribute coverage.
          for (const req of def.requiredAttributes) {
            if (!(req in source.emit.attributes)) {
              issues.push({
                ruleIndex,
                field: 'emit',
                severity: 'warning',
                message: `Target metric ${JSON.stringify(def.id)} requires attribute ${JSON.stringify(req)}; this rule does not set it`,
              });
            }
          }
          // Double-emit detection (only when harness is enabled).
          if (mapping.enabled) {
            const key = `${source.when}\x00${source.emit.metric}`;
            if (seenWhenMetric.has(key)) {
              issues.push({
                ruleIndex,
                field: 'emit',
                severity: 'error',
                message: `Another rule already fires on "${source.when}" emitting "${source.emit.metric}"; would double-count`,
              });
            }
            seenWhenMetric.add(key);
          }
        }
      }
    } else if (source.kind === 'synthesize-from-native') {
      if (!source.nativeMetric.trim()) {
        issues.push({
          ruleIndex,
          field: 'nativeMetric',
          severity: 'error',
          message: 'Native metric name is required',
        });
      }
      const def = catalogById.get(source.targetMetric);
      if (!def) {
        issues.push({
          ruleIndex,
          field: 'targetMetric',
          severity: 'error',
          message: `Target metric "${source.targetMetric}" is not in the catalog`,
        });
      } else {
        // Closed-enum check on inject attributes.
        for (const [k, v] of Object.entries(source.injectAttributes)) {
          const domain = CLOSED_ENUM_ATTRS[k];
          if (domain && !domain.includes(v)) {
            issues.push({
              ruleIndex,
              field: 'injectAttributes',
              severity: 'error',
              message: `Inject attribute ${k}=${JSON.stringify(v)} is not in the closed-enum domain {${domain.join(', ')}}`,
            });
          }
        }
        // Required-attribute coverage: each requiredAttribute must
        // either be injected as a constant, or be the target side of an
        // attributeMap rename.
        const mappedTargets = new Set(Object.values(source.attributeMap));
        for (const req of def.requiredAttributes) {
          const covered = req in source.injectAttributes || mappedTargets.has(req);
          if (!covered) {
            issues.push({
              ruleIndex,
              field: 'injectAttributes',
              severity: 'warning',
              message: `Target metric ${JSON.stringify(def.id)} requires attribute ${JSON.stringify(req)}; add it to inject attributes or rename a source attribute to it`,
            });
          }
        }
      }
    }
  });

  return issues;
}

/** Quick-check whether a rule index has any blocking errors. */
export function hasBlockingErrors(issues: RuleIssue[]): boolean {
  return issues.some((i) => i.severity === 'error');
}

/** Validate the metric catalog itself. Catches duplicate ids, bad
 *  identifier characters, builtin name overrides. */
export interface CatalogIssue {
  metricIndex: number;
  field: 'id' | 'name' | 'requiredAttributes';
  severity: 'error' | 'warning';
  message: string;
}

const ID_PATTERN = /^[a-z][a-z0-9._-]*$/;

export function validateCatalog(catalog: TroveMetricDefinition[]): CatalogIssue[] {
  const issues: CatalogIssue[] = [];
  const seenIds = new Map<string, number>();
  catalog.forEach((m, idx) => {
    if (!m.id.trim()) {
      issues.push({ metricIndex: idx, field: 'id', severity: 'error', message: 'ID is required' });
    } else if (!ID_PATTERN.test(m.id)) {
      issues.push({
        metricIndex: idx,
        field: 'id',
        severity: 'error',
        message:
          'ID must start with a lowercase letter and contain only lowercase letters, digits, dots, dashes, and underscores',
      });
    }
    if (!m.name.trim()) {
      issues.push({
        metricIndex: idx,
        field: 'name',
        severity: 'error',
        message: 'Wire name is required',
      });
    }
    const prev = seenIds.get(m.id);
    if (prev !== undefined) {
      issues.push({
        metricIndex: idx,
        field: 'id',
        severity: 'error',
        message: `Duplicate id; also used at row ${prev + 1}`,
      });
    } else {
      seenIds.set(m.id, idx);
    }
  });
  return issues;
}

/** Deep equality on a MappingState — used to clear the dirty pill when
 *  a draft equals its persisted form after edits cancel out. JSON.stringify
 *  is fine here: BTreeMap-backed records serialize to sorted keys on the
 *  Rust side, and Zod's `z.record` preserves insertion order in TS — to
 *  make this comparison order-stable, we re-key into sorted-key JSON
 *  strings before comparing.
 */
export function mappingStatesEqual(a: MappingState, b: MappingState): boolean {
  return canonicalize(a) === canonicalize(b);
}

function canonicalize(value: unknown): string {
  if (Array.isArray(value)) {
    return `[${value.map(canonicalize).join(',')}]`;
  }
  if (value && typeof value === 'object') {
    const keys = Object.keys(value as Record<string, unknown>).sort();
    return `{${keys
      .map((k) => `${JSON.stringify(k)}:${canonicalize((value as Record<string, unknown>)[k])}`)
      .join(',')}}`;
  }
  return JSON.stringify(value);
}

/** Helper exported for tests and the diff sheet — re-emit `state` with
 *  keys sorted recursively. JSON.stringify then produces a stable
 *  representation suitable for diffing. */
export function canonicalizeMappingState(state: MappingState): string {
  return canonicalize(state);
}

/** A safe "blank rule" constructor used by the [+ Add] buttons. */
export function blankSynthesisRule(catalog: TroveMetricDefinition[]): MappingSource {
  const fallbackTarget = catalog[0]?.id ?? 'events';
  return {
    kind: 'synthesize-from-native',
    nativeMetric: '',
    targetMetric: fallbackTarget,
    attributeMap: {},
    injectAttributes: {},
  };
}

export function blankHookRule(catalog: TroveMetricDefinition[]): MappingSource {
  const fallbackTarget = catalog[0]?.id ?? 'events';
  return {
    kind: 'hook-rule',
    when: '',
    emit: {
      metric: fallbackTarget,
      attributes: {},
    },
  };
}
