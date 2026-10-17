import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  LAUNCH_COMMAND_MAX,
  buildLaunchInput,
  defaultLaunch,
  sanitizeLaunchCommand,
  shouldInjectLaunch,
} from "../src/lib/launch.ts";

describe("launch: comando di avvio", () => {
  it("trim + limite 1000 caratteri", () => {
    assert.equal(LAUNCH_COMMAND_MAX, 1000);
    assert.equal(sanitizeLaunchCommand("  npm run dev  "), "npm run dev");
    assert.equal(sanitizeLaunchCommand(""), "");
    assert.equal(sanitizeLaunchCommand("x".repeat(1500)), "x".repeat(1000));
  });

  it("vuoto o blank => nessuna iniezione", () => {
    assert.equal(shouldInjectLaunch(""), false);
    assert.equal(shouldInjectLaunch("   \t  "), false);
    assert.equal(shouldInjectLaunch("ls"), true);
  });

  it("l'input inviato al pty termina con ritorno carrello", () => {
    assert.equal(buildLaunchInput("ls"), "ls\r");
  });

  it("defaultLaunch trova l'avvio predefinito o undefined", () => {
    const a = { id: "a", name: "A", command: "ls", is_default: false };
    const b = { id: "b", name: "B", command: "pwd", is_default: true };
    assert.equal(defaultLaunch({ launches: [a, b] }), b);
    assert.equal(defaultLaunch({ launches: [a] }), undefined);
    assert.equal(defaultLaunch({ launches: [] }), undefined);
    assert.equal(defaultLaunch({}), undefined);
  });
});
