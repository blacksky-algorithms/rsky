/** @type {import('tailwindcss').Config} */
module.exports = {
  mode: "all",
  darkMode: "class", // Ensure class-based dark mode is enabled
  content: ["./src/**/*.{rs,html,css}", "./dist/**/*.html"],
  theme: {
    extend: {},
  },
  plugins: [],
};
