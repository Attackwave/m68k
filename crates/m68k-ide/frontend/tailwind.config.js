/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {
      colors: {
        amiga: {
          blue: '#0055AA',
          orange: '#FF8800',
          darkblue: '#002255',
          grey: '#AAAAAA',
          black: '#000000',
          white: '#FFFFFF',
        },
        studio: {
          bg: '#0F1117',
          sidebar: '#161922',
          panel: '#1E222D',
          border: '#2A2F3D',
          hover: '#2D3344',
          accent: '#3B82F6',
          accentHover: '#2563EB',
          text: '#E2E8F0',
          muted: '#94A3B8',
        }
      },
      fontFamily: {
        mono: ['"JetBrains Mono"', '"Fira Code"', 'Consolas', 'monospace'],
        amiga: ['"Topaz Plus"', '"Courier New"', 'monospace'],
      }
    },
  },
  plugins: [],
}
