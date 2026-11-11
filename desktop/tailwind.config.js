/** @type {import('tailwindcss').Config} */
module.exports = {
  content: ['./index.html', './src/**/*.{js,ts,jsx,tsx}'],
  theme: {
    extend: {
      colors: {
        base: '#0a0e14',
        panel: '#10141c',
        panel2: '#161b26',
        panel3: '#1c2331',
        line: '#232c3d',
        lineSoft: '#1a2130',
        text: '#e6edf6',
        dim: '#8b95a9',
        accent: '#58d6ff',
        mint: '#7ee787',
        warn: '#f2cc60',
        danger: '#f87171',
        gradA: '#58d6ff',
        gradB: '#7ee787',
      },
      fontFamily: {
        sans: ['"Segoe UI"', 'Inter', 'system-ui', 'sans-serif'],
        mono: ['"Cascadia Code"', 'Consolas', 'monospace'],
      },
      boxShadow: {
        glow: '0 0 18px rgba(88, 214, 255, 0.35)',
        glowMint: '0 0 18px rgba(126, 231, 135, 0.35)',
        card: '0 12px 40px rgba(0, 0, 0, 0.45)',
      },
      backgroundImage: {
        'accent-grad': 'linear-gradient(135deg, #58d6ff 0%, #7ee787 100%)',
      },
    },
  },
  plugins: [],
};
