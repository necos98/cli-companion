import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { TERMINAL_THEME } from "../src/lib/terminalTheme.ts";

// Palette brand: ANSI armonizzati con l'accento ciano dell'app.
const BRAND = {
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

describe("terminale: tema ANSI", () => {
  it("espone tutti i 16 colori ANSI con valori della palette brand", () => {
    for (const [key, value] of Object.entries(BRAND)) {
      assert.equal(TERMINAL_THEME[key], value, `colore ${key}`);
    }
  });

  it("non usa le chiavi purple di Windows Terminal (xterm le ignorerebbe)", () => {
    assert.ok(!("purple" in TERMINAL_THEME));
    assert.ok(!("brightPurple" in TERMINAL_THEME));
  });

  it("definisce base, cursore e selezione", () => {
    for (const key of ["background", "foreground", "cursor"]) {
      assert.match(TERMINAL_THEME[key], /^#[0-9a-fA-F]{6}$/, key);
    }
    assert.equal(TERMINAL_THEME.background, "#0A0E12");
    assert.equal(TERMINAL_THEME.cursor, "#38BDF8");
    assert.match(TERMINAL_THEME.selectionBackground, /^rgba\(/);
  });
});
