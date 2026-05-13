export type DotStatus = 'green' | 'amber' | 'red' | 'gray';
export type DotSize = 'sm' | 'md';

export interface StatusDotProps {
  status: DotStatus;
  /** `sm` is 8px (header / inline use); `md` is 10px (in-row use). */
  size?: DotSize;
  /** Pulse the dot. Defaults to true for green, false otherwise. */
  pulse?: boolean;
  /** Accessible label / tooltip. */
  label?: string;
  /** Forwarded to the root as `data-testid`. */
  testid?: string;
  /** Extra `data-*` attrs (e.g. `data-health` on AppHeader). */
  dataAttrs?: Record<`data-${string}`, string>;
  className?: string;
}

const COLOR: Record<DotStatus, string> = {
  green: 'bg-ios-green',
  amber: 'bg-ios-orange',
  red: 'bg-ios-red',
  gray: 'bg-ios-gray',
};

const SIZE: Record<DotSize, string> = {
  sm: 'h-2 w-2',
  md: 'h-2.5 w-2.5',
};

/** Apple-style pulsing status dot. The `green` state pulses by default
 *  to convey "alive / healthy / receiving"; other statuses sit still so
 *  the user's attention is drawn to the steady-but-not-green ones. */
export function StatusDot({
  status,
  size = 'md',
  pulse,
  label,
  testid,
  dataAttrs,
  className = '',
}: StatusDotProps): JSX.Element {
  const shouldPulse = pulse ?? status === 'green';
  return (
    <span
      data-testid={testid}
      aria-label={label}
      title={label}
      className={`relative flex shrink-0 ${SIZE[size]} ${label ? 'cursor-help' : ''} ${className}`}
      {...dataAttrs}
    >
      {shouldPulse ? (
        <span
          aria-hidden="true"
          className={`absolute inline-flex h-full w-full rounded-full opacity-50 animate-ping ${COLOR[status]}`}
        />
      ) : null}
      <span
        aria-hidden="true"
        className={`relative inline-flex rounded-full ${SIZE[size]} ${COLOR[status]}`}
      />
    </span>
  );
}
