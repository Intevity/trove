import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { Feature } from '../data/features';

interface Props {
  features: Feature[];
  /** import.meta.env.BASE_URL passed from the Astro page (has a trailing slash). */
  base: string;
}

const AUTOPLAY_MS = 7000;

function prefersReducedMotion(): boolean {
  if (typeof window === 'undefined' || !window.matchMedia) return false;
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches;
}

/** The webm companion sits at the same basename as the mp4 (detect.mp4 -> detect.webm). */
function toWebm(video: string): string {
  return video.replace(/\.[^.]+$/, '.webm');
}

/**
 * Homepage feature carousel. Ported from Sentinel's video-driven carousel. When a
 * slide has hasVideo, its stage renders an autoplaying muted/looped <video> (mp4
 * with a webm companion) and clicking opens the clip full-size in the lightbox.
 * Slides without a recording fall back to the poster <img> plus a "Demo coming
 * soon" badge, and their lightbox shows the same coming-soon state. Auto-advance
 * runs on a fixed timer; hover pauses it. Reduced motion suppresses inline
 * autoplay (the poster frame shows instead). Data-driven from src/data/features.ts.
 */
export default function FeatureCarousel({ features, base }: Props) {
  const [active, setActive] = useState(0);
  const [paused, setPaused] = useState(false);
  // True while the lightbox is open (playing the clip, or the "coming soon"
  // state for a slide without one); holds the carousel in place.
  const [lightbox, setLightbox] = useState(false);
  const reduced = useMemo(prefersReducedMotion, []);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const closeBtnRef = useRef<HTMLButtonElement | null>(null);

  const asset = useCallback(
    (p: string) => `${base.replace(/\/$/, '')}/videos/${p.replace(/^\//, '')}`,
    [base],
  );

  const advance = useCallback(() => {
    setActive((i) => (i + 1) % features.length);
  }, [features.length]);

  // A slide click opens the lightbox for the active slide: it plays the clip
  // full-size when the slide has a recording, or shows a "coming soon" state
  // when it does not.
  const openLightbox = useCallback(() => {
    setLightbox(true);
  }, []);

  // Auto-advance on a fixed timer. Paused on hover, while the lightbox is open,
  // when reduced motion is requested, or with a single slide.
  useEffect(() => {
    if (reduced || paused || lightbox || features.length <= 1) return;
    timer.current = setTimeout(advance, AUTOPLAY_MS);
    return () => {
      if (timer.current) clearTimeout(timer.current);
    };
  }, [reduced, paused, lightbox, features.length, active, advance]);

  // Close the lightbox on Escape and move focus to its close button on open.
  useEffect(() => {
    if (!lightbox) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setLightbox(false);
    };
    window.addEventListener('keydown', onKey);
    closeBtnRef.current?.focus();
    return () => window.removeEventListener('keydown', onKey);
  }, [lightbox]);

  const current = features[active];

  return (
    <div
      className="mx-auto w-full max-w-5xl"
      onMouseEnter={() => setPaused(true)}
      onMouseLeave={() => setPaused(false)}
    >
      {/* Tab rail */}
      <div
        className="mb-6 flex flex-wrap items-center justify-center gap-2"
        role="tablist"
        aria-label="Feature demos"
      >
        {features.map((f, i) => {
          const selected = i === active;
          return (
            <button
              key={f.slug}
              type="button"
              role="tab"
              aria-selected={selected}
              onClick={() => setActive(i)}
              className={[
                'rounded-full px-4 py-2 text-sm font-semibold transition-all active:scale-95',
                selected
                  ? 'bg-brand text-white shadow-card'
                  : 'border border-border-subtle/15 text-foreground/70 hover:bg-foreground/5',
              ].join(' ')}
            >
              {f.label}
            </button>
          );
        })}
      </div>

      {/* Stage */}
      <div className="glass-card overflow-hidden">
        <button
          type="button"
          onClick={openLightbox}
          aria-label={
            current.hasVideo
              ? `Play the ${current.title} demo at full size`
              : `${current.title} — demo coming soon`
          }
          className="group relative block aspect-video w-full overflow-hidden bg-[#0f1413]"
        >
          {/* Media: an autoplaying muted/looped clip when the slide has a
              recording, otherwise the poster placeholder. */}
          {current.hasVideo ? (
            <video
              key={current.slug}
              poster={asset(current.poster)}
              muted
              playsInline
              loop
              autoPlay={!reduced}
              preload="auto"
              aria-label={`${current.title} demo`}
              className="absolute inset-0 h-full w-full object-cover"
            >
              <source src={asset(toWebm(current.video))} type="video/webm" />
              <source src={asset(current.video)} type="video/mp4" />
            </video>
          ) : (
            <img
              key={current.slug}
              src={asset(current.poster)}
              alt={`${current.title} preview`}
              loading="lazy"
              decoding="async"
              className="absolute inset-0 h-full w-full object-cover"
            />
          )}

          {/* Accent wash tuned to the slide's teal accent. */}
          <span
            aria-hidden="true"
            className="pointer-events-none absolute inset-0 opacity-50 transition-opacity group-hover:opacity-70"
            style={{
              background: `radial-gradient(60% 60% at 50% 0%, ${current.accent}33, transparent 70%)`,
            }}
          />

          {/* Click affordance: an expand glyph for slides with a real clip, the
              "Demo coming soon" badge for poster-only slides. */}
          {current.hasVideo ? (
            <span className="pointer-events-none absolute left-1/2 top-1/2 inline-flex h-12 w-12 -translate-x-1/2 -translate-y-1/2 items-center justify-center rounded-full bg-white/12 text-white opacity-0 ring-1 ring-white/25 backdrop-blur-sm transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100">
              <svg
                width="20"
                height="20"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
              >
                <path d="M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7" />
              </svg>
            </span>
          ) : (
            <span className="pointer-events-none absolute left-1/2 top-1/2 inline-flex -translate-x-1/2 -translate-y-1/2 items-center gap-2 rounded-full bg-white/12 px-4 py-2 text-sm font-semibold text-white ring-1 ring-white/25 backdrop-blur-sm transition-colors group-hover:bg-white/20">
              <svg
                width="18"
                height="18"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
              >
                <circle cx="12" cy="12" r="10" />
                <polyline points="12 6 12 12 16 14" />
              </svg>
              Demo coming soon
            </span>
          )}

          {/* Caption */}
          <span className="pointer-events-none absolute inset-x-0 bottom-0 block bg-gradient-to-t from-black/80 to-transparent p-5 text-left">
            <span className="block text-base font-semibold text-white sm:text-lg">
              {current.title}
            </span>
            <span className="mt-1 block max-w-2xl text-sm text-white/70">{current.tagline}</span>
          </span>
        </button>
      </div>

      {/* Description under the stage. */}
      <p className="mx-auto mt-5 max-w-2xl text-center text-sm leading-relaxed text-muted">
        {current.description}
      </p>

      {/* Progress dots */}
      <div className="mt-5 flex items-center justify-center gap-2" aria-hidden="true">
        {features.map((f, i) => (
          <span
            key={f.slug}
            className={[
              'h-1.5 rounded-full transition-all',
              i === active ? 'w-6 bg-brand' : 'w-1.5 bg-foreground/20',
            ].join(' ')}
          />
        ))}
      </div>

      {/* Lightbox: graceful "coming soon" state in place of a player. Closes on
          backdrop click, the X button, or Escape. */}
      {lightbox && (
        <div
          className="fixed inset-0 z-[60] flex items-center justify-center p-3 sm:p-8"
          role="dialog"
          aria-modal="true"
          aria-label={`${current.title} demo`}
        >
          <button
            type="button"
            aria-label="Close"
            onClick={() => setLightbox(false)}
            className="absolute inset-0 cursor-default bg-black/75 backdrop-blur-sm"
          />
          <div className="relative flex w-[min(900px,94vw)] flex-col items-center">
            <button
              ref={closeBtnRef}
              type="button"
              aria-label="Close"
              onClick={() => setLightbox(false)}
              className="absolute -top-11 right-0 inline-flex h-9 w-9 items-center justify-center rounded-full bg-white/15 text-white transition-colors hover:bg-white/25 sm:-top-12"
            >
              <svg
                width="22"
                height="22"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
              >
                <path d="M18 6 6 18M6 6l12 12" />
              </svg>
            </button>
            {current.hasVideo ? (
              <video
                key={current.slug}
                poster={asset(current.poster)}
                className="max-h-[78vh] w-full rounded-2xl bg-black object-contain shadow-2xl"
                controls
                autoPlay
                muted
                loop
                playsInline
              >
                <source src={asset(toWebm(current.video))} type="video/webm" />
                <source src={asset(current.video)} type="video/mp4" />
              </video>
            ) : (
              <div className="relative w-full overflow-hidden rounded-2xl bg-black shadow-2xl">
                <img
                  src={asset(current.poster)}
                  alt={`${current.title} preview`}
                  className="max-h-[78vh] w-full object-contain"
                />
                <div className="pointer-events-none absolute inset-0 flex items-center justify-center">
                  <span className="inline-flex items-center gap-2 rounded-full bg-white/12 px-4 py-2 text-sm font-semibold text-white ring-1 ring-white/25 backdrop-blur-sm">
                    <svg
                      width="18"
                      height="18"
                      viewBox="0 0 24 24"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="2"
                      strokeLinecap="round"
                      strokeLinejoin="round"
                    >
                      <circle cx="12" cy="12" r="10" />
                      <polyline points="12 6 12 12 16 14" />
                    </svg>
                    Demo coming soon
                  </span>
                </div>
              </div>
            )}
            <p className="mt-3 text-center text-sm font-semibold text-white/85">{current.title}</p>
          </div>
        </div>
      )}
    </div>
  );
}
