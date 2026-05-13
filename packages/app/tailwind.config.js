/** @type {import('tailwindcss').Config} */
export default {
  content: ['./index.html', './src/**/*.{js,ts,jsx,tsx}'],
  darkMode: 'media',
  theme: {
    extend: {
      colors: {
        // Sidecar / harness health palette — Sprint 6 wires the dynamic tray
        // retinting against this scale.
        health: {
          green: '#10b981',
          amber: '#f59e0b',
          red: '#ef4444',
          gray: '#94a3b8',
        },
        // iOS system palette used by the shell chrome (header, tabs, footer).
        'ios-blue': '#007AFF',
        'ios-blue-hover': '#0066CC',
        'ios-green': '#32D74B',
        'ios-red': '#FF453A',
        'ios-orange': '#FF9F0A',
        'ios-gray': '#8E8E93',

        // macOS-native surface tokens. Light + dark are sibling utility
        // names rather than DEFAULT/dark because most use sites read
        // cleaner as `bg-canvas dark:bg-canvas-dark`.
        canvas: '#f2f2f7',
        'canvas-dark': '#1c1c1e',
        surface: '#ffffff',
        'surface-dark': '#1c1c1e',
        'surface-elevated': '#ffffff',
        'surface-elevated-dark': '#2c2c2e',

        // Foreground hierarchy — opacity-based, mirroring AppKit's
        // labelColor / secondaryLabelColor / tertiaryLabelColor /
        // quaternaryLabelColor semantics.
        'fg-primary': '#1d1d1f',
        'fg-primary-dark': '#f5f5f7',
        'fg-secondary': 'rgba(0, 0, 0, 0.60)',
        'fg-secondary-dark': 'rgba(255, 255, 255, 0.65)',
        'fg-tertiary': 'rgba(0, 0, 0, 0.42)',
        'fg-tertiary-dark': 'rgba(255, 255, 255, 0.48)',
        'fg-quaternary': 'rgba(0, 0, 0, 0.26)',
        'fg-quaternary-dark': 'rgba(255, 255, 255, 0.30)',

        // 1px hairline borders — the same value the header already uses
        // (`border-black/10 dark:border-white/10`), now as a token so
        // every surface in the app picks the same line.
        hairline: 'rgba(0, 0, 0, 0.10)',
        'hairline-dark': 'rgba(255, 255, 255, 0.12)',
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
        // Large prominent numerals (Collector tile, KPI display).
        display: ['28px', { lineHeight: '1.05', letterSpacing: '-0.02em', fontWeight: '600' }],
        // Uppercase caption labels under display numerals.
        caption: ['10px', { lineHeight: '1.2', letterSpacing: '0.08em' }],
      },
      borderRadius: {
        card: '10px',
        tile: '12px',
        modal: '16px',
        pill: '999px',
      },
      boxShadow: {
        // Apple's elevation recipe: a hairline ring + a 1px drop. The
        // ring is what gives surfaces their crisp edge against the
        // canvas; the drop is barely perceptible but separates the
        // surface from a flat-on-flat read.
        card: '0 1px 2px rgba(0, 0, 0, 0.04), 0 0 0 0.5px rgba(0, 0, 0, 0.06)',
        'card-dark': '0 1px 2px rgba(0, 0, 0, 0.5), 0 0 0 0.5px rgba(255, 255, 255, 0.08)',
        modal: '0 24px 56px -16px rgba(0, 0, 0, 0.30), 0 0 0 0.5px rgba(0, 0, 0, 0.06)',
        'modal-dark': '0 24px 56px -16px rgba(0, 0, 0, 0.6), 0 0 0 0.5px rgba(255, 255, 255, 0.08)',
      },
    },
  },
  plugins: [],
};
