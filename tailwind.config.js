/** @type {import('tailwindcss').Config} */
module.exports = {
  content: [
    "./templates/**/*.html",
    "./src/**/*.rs",
  ],
  theme: {
    extend: {
      colors: {
        slate: {
          950: '#0f121a',
        }
      },
      animation: {
        'spin-slow': 'spin 2s linear infinite',
      }
    },
  },
  plugins: [],
}
