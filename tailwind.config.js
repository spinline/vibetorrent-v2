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
          950: '#0a0e14',
        }
      },
      animation: {
        'spin-slow': 'spin 2s linear infinite',
      }
    },
  },
  plugins: [],
}
