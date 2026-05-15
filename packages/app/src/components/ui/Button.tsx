import type { ButtonHTMLAttributes, ReactNode } from 'react';

export type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'destructive';
export type ButtonSize = 'sm' | 'md';

export interface ButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'children'> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  /** Disabled + shows a spinner-less "in progress" affordance. Caller
   *  swaps the label text (e.g. "Save" → "Saving…") to match. */
  loading?: boolean;
  /** Forwarded as `data-testid`. Separate from `name` to keep the React
   *  prop semantics clean. */
  testid?: string;
  children: ReactNode;
}

const BASE =
  'inline-flex items-center justify-center gap-1.5 rounded-[8px] font-medium tracking-tight transition-colors duration-150 disabled:opacity-50 disabled:cursor-not-allowed focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand focus-visible:ring-offset-1 focus-visible:ring-offset-canvas dark:focus-visible:ring-offset-canvas-dark';

const VARIANT: Record<ButtonVariant, string> = {
  primary: 'bg-brand text-white hover:bg-brand-hover',
  secondary:
    'bg-black/[0.04] text-fg-primary hover:bg-black/[0.08] dark:bg-white/[0.06] dark:text-fg-primary-dark dark:hover:bg-white/[0.10]',
  ghost:
    'bg-transparent text-brand hover:underline underline-offset-2 hover:bg-brand/[0.06] dark:hover:bg-brand/[0.12]',
  destructive: 'bg-ios-red text-white hover:opacity-90',
};

const SIZE: Record<ButtonSize, string> = {
  sm: 'text-[12px] px-2.5 py-1',
  md: 'text-[13px] px-3 py-1.5',
};

/** macOS-native Button. Variants are normalized — primary is iOS blue,
 *  secondary is the translucent neutral chip, ghost is text-only blue,
 *  destructive is iOS red. Sizes are sm (chips, inline actions) and md
 *  (form submit buttons). */
export function Button({
  variant = 'secondary',
  size = 'sm',
  loading = false,
  testid,
  className = '',
  disabled,
  children,
  type = 'button',
  ...rest
}: ButtonProps): JSX.Element {
  const classes = [BASE, VARIANT[variant], SIZE[size], className].filter(Boolean).join(' ');
  return (
    <button
      type={type}
      className={classes}
      data-testid={testid}
      disabled={disabled || loading}
      {...rest}
    >
      {children}
    </button>
  );
}
