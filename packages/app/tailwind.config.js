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
    },
  },
  plugins: [],
};
