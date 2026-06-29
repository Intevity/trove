import { defineConfig } from 'astro/config';
import react from '@astrojs/react';
import starlight from '@astrojs/starlight';
import tailwind from '@astrojs/tailwind';

const repo = 'https://github.com/Intevity/trove';

// GitHub Pages project site. The repo deploys to
// https://intevity.github.io/trove/, so `base` must be the repo
// name; every internal link/asset is resolved against `import.meta.env.BASE_URL`
// (which is `/trove/`). A custom domain would set base back to '/'.
//
// Starlight powers the documentation at `/trove/docs/`. It coexists with the
// hand-built marketing homepage (`src/pages/index.astro`, route `/trove/`):
// every docs page lives under `src/content/docs/docs/**`, so its slug starts with
// `docs/` and never competes for the site root. The marketing landing page keeps
// the root; Starlight only owns `/docs/**`.
export default defineConfig({
  site: 'https://intevity.github.io',
  base: '/trove',
  output: 'static',
  integrations: [
    starlight({
      title: 'Trove docs',
      description:
        'Documentation for Trove — the open-source tray app that unifies OpenTelemetry from every AI coding tool on your machine and forwards one normalized stream to the observability backend you choose.',
      logo: {
        src: './src/assets/trove-logo.svg',
        alt: 'Trove',
      },
      favicon: '/favicon.png',
      social: [{ icon: 'github', label: 'GitHub', href: repo }],
      editLink: {
        baseUrl: `${repo}/edit/main/packages/site/`,
      },
      // Brand the default Starlight theme to match the marketing site
      // (Trove teal accent, dark default). See src/styles/starlight.css.
      customCss: ['./src/styles/starlight.css'],
      // The marketing homepage owns the site root, so send the Starlight
      // masthead title/logo back to it rather than to a docs landing.
      // Sidebar groups map 1:1 to the content directories under
      // `src/content/docs/docs/`; per-page order is set via frontmatter
      // (`sidebar.order`) where alphabetical is wrong.
      sidebar: [
        { label: 'Overview', slug: 'docs' },
        {
          label: 'Getting Started',
          autogenerate: { directory: 'docs/getting-started' },
        },
        {
          label: 'Features',
          autogenerate: { directory: 'docs/features' },
        },
        {
          label: 'Guides',
          autogenerate: { directory: 'docs/guides' },
        },
        {
          label: 'Reference',
          autogenerate: { directory: 'docs/reference' },
        },
        {
          label: 'Developers',
          autogenerate: { directory: 'docs/developers' },
        },
      ],
    }),
    react(),
    // We own the full marketing stylesheet (src/styles/global.css ships its own
    // @tailwind directives, theme tokens, and component layer), so disable the
    // integration's injected base to avoid a duplicate preflight. Tailwind only
    // applies to pages that import global.css (the marketing layout) — Starlight
    // docs pages use Starlight's own styles and are unaffected.
    tailwind({ applyBaseStyles: false }),
  ],
});
