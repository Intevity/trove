export interface HairlineProps {
  vertical?: boolean;
  className?: string;
}

/** A single-pixel hairline divider. `vertical` flips to a `w-px h-full`
 *  shape; the default is horizontal `h-px w-full`. */
export function Hairline({ vertical = false, className = '' }: HairlineProps): JSX.Element {
  const shape = vertical ? 'w-px h-full' : 'h-px w-full';
  return (
    <span
      aria-hidden="true"
      className={`block bg-hairline dark:bg-hairline-dark ${shape} ${className}`}
    />
  );
}
