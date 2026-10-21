module.exports = {
    plugins: {
      '@tailwindcss/postcss': {},
      // Tailwind 4 prefixes only the CSS it compiles; the plain stylesheets still need this.
      autoprefixer: {},
    },
  }