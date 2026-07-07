/** @type {import('tailwindcss').Config} */
// Mirrors packages/app/tailwind.config.js so a component dropped from the app
// renders identically on the web. Keep the brand teal, ios.* palette, font
// stack, radius, and shadow scale in sync with the app. The marketing-only
// extras (the clamp-based `display` headline size and the `2xs` label size)
// are layered on top for the landing page.
export default {
  content: ['./src/**/*.{astro,html,js,jsx,ts,tsx,md,mdx}', './public/**/*.{svg,html}'],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        // Trove brand teal — primary marketing accent (replaces Sentinel's
        // iOS-blue). Mirrored in the app's tray retint (TintColor::Brand).
        brand: '#2dbfb8',
        'brand-hover': '#26a8a2',

        // iOS system palette used by the shell chrome (header, tabs, footer)
        // and carried into the marketing surfaces for parity with the app.
        ios: {
          blue: '#007AFF',
          green: '#32D74B',
          orange: '#FF9F0A',
          red: '#FF453A',
          purple: '#BF5AF2',
          indigo: '#5E5CE6',
          gray: '#8E8E93',
        },

        // Sidecar / harness health palette — matches the app so health pills
        // and coverage badges read identically on the web.
        health: {
          green: '#10b981',
          amber: '#f59e0b',
          red: '#ef4444',
          gray: '#94a3b8',
        },

        // macOS-native surface tokens. Light + dark are sibling utility names
        // (e.g. `bg-canvas dark:bg-canvas-dark`).
        canvas: '#f2f2f7',
        'canvas-dark': '#1c1c1e',
        surface: '#ffffff',
        'surface-dark': '#1c1c1e',
        'surface-elevated': '#ffffff',
        'surface-elevated-dark': '#2c2c2e',

        // Foreground hierarchy — opacity-based, mirroring AppKit's
        // labelColor / secondaryLabelColor / tertiaryLabelColor semantics.
        'fg-primary': '#1d1d1f',
        'fg-primary-dark': '#f5f5f7',
        'fg-secondary': 'rgba(0, 0, 0, 0.60)',
        'fg-secondary-dark': 'rgba(255, 255, 255, 0.65)',
        'fg-tertiary': 'rgba(0, 0, 0, 0.42)',
        'fg-tertiary-dark': 'rgba(255, 255, 255, 0.48)',
        'fg-quaternary': 'rgba(0, 0, 0, 0.26)',
        'fg-quaternary-dark': 'rgba(255, 255, 255, 0.30)',

        // 1px hairline borders.
        hairline: 'rgba(0, 0, 0, 0.10)',
        'hairline-dark': 'rgba(255, 255, 255, 0.12)',

        // Semantic theme tokens backed by CSS vars in src/styles/global.css
        // (created by a later agent). These let marketing components read
        // `text-foreground`, `text-muted`, etc.
        foreground: 'rgb(var(--foreground) / <alpha-value>)',
        muted: 'rgb(var(--muted) / <alpha-value>)',
        'border-subtle': 'rgb(var(--border-subtle) / <alpha-value>)',
        'surface-overlay': 'rgb(var(--surface-overlay) / <alpha-value>)',
      },
      fontFamily: {
        sans: [
          '-apple-system',
          'BlinkMacSystemFont',
          "'SF Pro Text'",
          "'Segoe UI'",
          "'Helvetica Neue'",
          'sans-serif',
        ],
      },
      fontSize: {
        // Marketing-only extras (from Sentinel's site config).
        '2xs': ['10px', { lineHeight: '14px' }],
        // Large clamp-based hero headline for the landing page.
        display: ['clamp(2.75rem, 6vw, 4.5rem)', { lineHeight: '1.05', letterSpacing: '-0.02em' }],
        // Uppercase caption labels (ported from the app).
        caption: ['10px', { lineHeight: '1.2', letterSpacing: '0.08em' }],
      },
      borderRadius: {
        card: '10px',
        tile: '12px',
        modal: '16px',
        pill: '999px',
        '2xl': '16px',
        '3xl': '22px',
      },
      boxShadow: {
        // App parity (Apple elevation recipe: hairline ring + 1px drop).
        card: '0 1px 2px rgba(0, 0, 0, 0.04), 0 0 0 0.5px rgba(0, 0, 0, 0.06)',
        'card-dark': '0 1px 2px rgba(0, 0, 0, 0.5), 0 0 0 0.5px rgba(255, 255, 255, 0.08)',
        modal: '0 24px 56px -16px rgba(0, 0, 0, 0.30), 0 0 0 0.5px rgba(0, 0, 0, 0.06)',
        'modal-dark': '0 24px 56px -16px rgba(0, 0, 0, 0.6), 0 0 0 0.5px rgba(255, 255, 255, 0.08)',
        // Marketing extras (softer elevations for cards/sticky chrome).
        'card-md': '0 4px 20px rgba(0,0,0,0.10), 0 0 0 0.5px rgba(0,0,0,0.05)',
        sticky:
          '0 8px 24px rgba(0,0,0,0.22), 0 2px 8px rgba(0,0,0,0.12), 0 0 0 0.5px rgba(0,0,0,0.06)',
        'sticky-dark': '0 10px 28px rgba(0,0,0,0.70), 0 2px 8px rgba(0,0,0,0.50)',
      },
      backdropBlur: {
        xs: '4px',
      },
    },
  },
  plugins: [],
};
