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
        'ios-green': '#32D74B',
        'ios-red': '#FF453A',
        'ios-orange': '#FF9F0A',
        'ios-gray': '#8E8E93',
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
