// Tema del terminale xterm (palette brand cli-companion, ANSI armonizzati
// con l'accento ciano).
// File volutamente a zero dipendenze: importabile sia dall'app sia dai test
// `node --test` (type-stripping), come src/lib/terminal.ts e src/lib/updater.ts.
//
// Nota: xterm.js vuole le chiavi `magenta`/`brightMagenta` (non `purple`):
// le chiavi ignote vengono ignorate in silenzio, lasciando i default.

/**
 * Sottoinsieme di `ITheme` di xterm.js usato dall'app.
 * Tipizzato qui per non dipendere da xterm nei test.
 */
export interface TerminalTheme {
  background: string;
  foreground: string;
  cursor: string;
  selectionBackground: string;
  black: string;
  red: string;
  green: string;
  yellow: string;
  blue: string;
  magenta: string;
  cyan: string;
  white: string;
  brightBlack: string;
  brightRed: string;
  brightGreen: string;
  brightYellow: string;
  brightBlue: string;
  brightMagenta: string;
  brightCyan: string;
  brightWhite: string;
}

/** Base scura dell'app (#0A0E12) + 16 ANSI in tono con la palette. */
export const TERMINAL_THEME: TerminalTheme = {
  background: "#0A0E12",
  foreground: "#C4CED8",
  cursor: "#38BDF8",
  selectionBackground: "rgba(56, 189, 248, 0.25)",
  black: "#1C2530",
  red: "#F87171",
  green: "#34D399",
  yellow: "#F5B544",
  blue: "#60A5FA",
  magenta: "#C084FC",
  cyan: "#38BDF8",
  white: "#C4CED8",
  brightBlack: "#5D6B79",
  brightRed: "#FCA5A5",
  brightGreen: "#6EE7B7",
  brightYellow: "#FCD34D",
  brightBlue: "#93C5FD",
  brightMagenta: "#D8B4FE",
  brightCyan: "#7DD3FC",
  brightWhite: "#E6EEF5",
};
