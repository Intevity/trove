import type { ReactNode } from 'react';

export type CardPadding = 'none' | 'sm' | 'md' | 'lg';

export interface CardProps {
  /** HTML element to render. Defaults to `section`. */
  as?: 'section' | 'div' | 'aside' | 'article';
  /** Forwarded to the root as `data-testid`. */
  testid?: string;
  /** `data-status` etc. — opaque pass-through for tests that look up
   *  status off the card root. */
  dataAttrs?: Record<`data-${string}`, string>;
  /** Padding scale (default `md`). `none` ships an unpadded surface
   *  for callers that own their internal layout. */
  padding?: CardPadding;
  /** Optional class extensions (layout-only — flex, sizing). Do not
   *  override colors/borders; that's the whole point of the primitive. */
  className?: string;
  children?: ReactNode;
}

const PADDING_CLASS: Record<CardPadding, string> = {
  none: '',
  sm: 'px-3 py-2',
  md: 'px-4 py-3',
  lg: 'px-6 py-5',
};

/** macOS-native card surface — hairline ring + 1px drop, soft radius,
 *  light/dark surface fills. The visual baseline that every section in
 *  the dashboard re-uses. */
export function Card({
  as: Tag = 'section',
  testid,
  dataAttrs,
  padding = 'md',
  className = '',
  children,
}: CardProps): JSX.Element {
  const classes = [
    'rounded-card border border-hairline bg-surface shadow-card',
    'dark:border-hairline-dark dark:bg-surface-dark dark:shadow-card-dark',
    PADDING_CLASS[padding],
    className,
  ]
    .filter(Boolean)
    .join(' ');
  return (
    <Tag className={classes} data-testid={testid} {...dataAttrs}>
      {children}
    </Tag>
  );
}
