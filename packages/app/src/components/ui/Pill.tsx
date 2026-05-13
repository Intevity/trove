import type { ReactNode } from 'react';

export type PillTone = 'neutral' | 'green' | 'amber' | 'red' | 'blue';
export type PillSize = 'xs' | 'sm';

export interface PillProps {
  tone?: PillTone;
  size?: PillSize;
  className?: string;
  testid?: string;
  children: ReactNode;
}

const TONE: Record<PillTone, string> = {
  neutral: 'bg-black/[0.06] text-fg-secondary dark:bg-white/[0.08] dark:text-fg-secondary-dark',
  green: 'bg-ios-green/[0.14] text-ios-green',
  amber: 'bg-ios-orange/[0.14] text-ios-orange',
  red: 'bg-ios-red/[0.14] text-ios-red',
  blue: 'bg-ios-blue/[0.14] text-ios-blue',
};

const SIZE: Record<PillSize, string> = {
  xs: 'text-[10px] px-2 py-0.5',
  sm: 'text-[11px] px-2.5 py-1',
};

/** Translucent status chip — used in place of fully-tinted row
 *  backgrounds. The chip carries the status; the surrounding surface
 *  stays neutral. */
export function Pill({
  tone = 'neutral',
  size = 'xs',
  className = '',
  testid,
  children,
}: PillProps): JSX.Element {
  const classes = [
    'inline-flex items-center gap-1 rounded-pill font-medium tracking-wide',
    TONE[tone],
    SIZE[size],
    className,
  ]
    .filter(Boolean)
    .join(' ');
  return (
    <span className={classes} data-testid={testid}>
      {children}
    </span>
  );
}
