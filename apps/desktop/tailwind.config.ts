import type { Config } from "tailwindcss";

export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        obsidian: "#0E0F13",
        slate: "#171A21",
        ash: "#22262F",
        fog: "#9AA3B2",
        mist: "#E7EBF2",
        sheen: "#4C82FB",
        iris: "#6D5EF3",
        halo: "#38BDF8",
        coral: "#F2785C",
      },
      fontFamily: {
        display: ['"Clash Display"', "ui-sans-serif", "system-ui", "sans-serif"],
        sans: ["Inter", "ui-sans-serif", "system-ui", "sans-serif"],
        mono: ['"JetBrains Mono"', "ui-monospace", "monospace"],
      },
      fontSize: {
        xs: ["0.75rem", { lineHeight: "1rem" }],
        sm: ["0.875rem", { lineHeight: "1.25rem" }],
        base: ["1rem", { lineHeight: "1.5rem" }],
        lg: ["1.25rem", { lineHeight: "1.75rem" }],
        xl: ["1.75rem", { lineHeight: "2.25rem" }],
        "2xl": ["2.5rem", { lineHeight: "3rem" }],
      },
      fontWeight: {
        normal: "400",
        medium: "500",
        semibold: "600",
      },
    },
  },
  plugins: [],
} satisfies Config;
