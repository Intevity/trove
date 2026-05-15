import { ExternalLink } from 'lucide-react';
import { useState } from 'react';

import type { HarnessId } from '@trove/shared';

import { Button, Pill, Sheet } from './ui/index.js';

export interface SetupGuideCopyValue {
  label: string;
  value: string;
}

export interface SetupGuideStep {
  title: string;
  body: string;
  copy?: SetupGuideCopyValue;
}

export interface SetupGuide {
  title: string;
  subtitle: string;
  intro: string;
  requirementPills?: string[];
  steps: SetupGuideStep[];
  docsUrl: string;
  docsLabel: string;
  /** Optional closing note, rendered as a muted line below the steps. */
  footnote?: string;
  /**
   * Label shown on the Harnesses-row CTA that opens this guide.
   * Defaults to "Set up →" — override for harnesses that need no
   * setup (e.g. auto-detected adapterless ones) where the guide is
   * informational rather than actionable.
   */
  buttonLabel?: string;
}

export const HARNESS_SETUP_GUIDES: Partial<Record<HarnessId, SetupGuide>> = {};

export interface HarnessSetupGuideModalProps {
  guide: SetupGuide;
  open: boolean;
  onClose: () => void;
  testid?: string;
}

export function HarnessSetupGuideModal({
  guide,
  open,
  onClose,
  testid,
}: HarnessSetupGuideModalProps): JSX.Element {
  return (
    <Sheet
      open={open}
      onClose={onClose}
      size="lg"
      title={guide.title}
      subtitle={guide.subtitle}
      {...(testid ? { testid } : {})}
      footer={
        <>
          <a
            href={guide.docsUrl}
            target="_blank"
            rel="noreferrer noopener"
            className="mr-auto inline-flex items-center gap-1 text-[12px] text-brand hover:underline"
          >
            <ExternalLink size={12} aria-hidden="true" />
            {guide.docsLabel}
          </a>
          <Button variant="primary" size="md" onClick={onClose}>
            Done
          </Button>
        </>
      }
    >
      <div className="space-y-4 px-5 py-4">
        <p
          className="text-[13px] text-fg-secondary dark:text-fg-secondary-dark"
          data-testid="setup-guide-intro"
        >
          {guide.intro}
        </p>

        {guide.requirementPills && guide.requirementPills.length > 0 ? (
          <div className="flex flex-wrap gap-1.5">
            {guide.requirementPills.map((req) => (
              <Pill key={req} tone="amber" size="xs">
                {req}
              </Pill>
            ))}
          </div>
        ) : null}

        <ol className="space-y-2.5" data-testid="setup-guide-steps">
          {guide.steps.map((step, i) => (
            <li
              key={i}
              className="flex gap-3 rounded-card border border-hairline bg-surface px-3 py-2.5 dark:border-hairline-dark dark:bg-surface-dark"
            >
              <span
                aria-hidden="true"
                className="mt-0.5 inline-flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-brand/15 text-[11px] font-semibold text-brand"
              >
                {i + 1}
              </span>
              <div className="min-w-0 flex-1">
                <p className="text-[13px] font-medium text-fg-primary dark:text-fg-primary-dark">
                  {step.title}
                </p>
                <p className="mt-0.5 text-[12px] text-fg-secondary dark:text-fg-secondary-dark">
                  {step.body}
                </p>
                {step.copy ? (
                  <CopyRow label={step.copy.label} value={step.copy.value} index={i} />
                ) : null}
              </div>
            </li>
          ))}
        </ol>

        {guide.footnote ? (
          <p className="text-[11px] italic text-fg-tertiary dark:text-fg-tertiary-dark">
            {guide.footnote}
          </p>
        ) : null}
      </div>
    </Sheet>
  );
}

interface CopyRowProps {
  label: string;
  value: string;
  index: number;
}

function CopyRow({ label, value, index }: CopyRowProps): JSX.Element {
  const [copied, setCopied] = useState(false);

  const handleClick = async (): Promise<void> => {
    const ok = await copyToClipboard(value);
    if (ok) {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    }
  };

  return (
    <div className="mt-2 flex items-center gap-2">
      <span className="text-[10px] uppercase tracking-wide text-fg-tertiary dark:text-fg-tertiary-dark">
        {label}
      </span>
      <code
        className="flex-1 truncate rounded-[6px] border border-hairline bg-canvas px-2 py-1 font-mono text-[12px] text-fg-primary dark:border-hairline-dark dark:bg-canvas-dark dark:text-fg-primary-dark"
        data-testid={`setup-guide-copy-value-${index}`}
      >
        {value}
      </code>
      <button
        type="button"
        onClick={() => void handleClick()}
        data-testid={`setup-guide-copy-${index}`}
        className="rounded-[6px] border border-brand/40 bg-surface px-2 py-1 text-[11px] font-medium text-brand hover:bg-brand/[0.10] dark:bg-surface-dark"
        aria-label={`Copy ${label}`}
      >
        {copied ? 'Copied' : 'Copy'}
      </button>
    </div>
  );
}

async function copyToClipboard(value: string): Promise<boolean> {
  if (typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(value);
      return true;
    } catch {
      // fall through; clipboard can fail in non-secure contexts
    }
  }
  return false;
}
