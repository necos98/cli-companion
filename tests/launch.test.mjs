import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  LAUNCH_COMMAND_MAX,
  buildLaunchInput,
  SHELL_LAUNCH_ID,
  allLaunches,
  launchCommands,
  resolveDefaultLaunch,
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

  const a = { id: "a", name: "A", command: "ls" };
  const b = { id: "b", name: "B", command: "pwd" };
  const cc = {
    id: "builtin:claude-code",
    name: "Claude Code",
    command: "claude",
    restore_command: "claude --resume",
    agent_id: "claude-code",
  };
  const orch = { ...cc, id: "builtin:preset:task-intake", name: "Task Intake" };

  it("allLaunches: prima gli integrati, poi i personalizzati", () => {
    assert.deepEqual(allLaunches({ launches: [a, b] }, [cc]), [cc, a, b]);
    assert.deepEqual(allLaunches({}, []), []);
  });

  it("resolveDefaultLaunch: workspace > globale > nessuno", () => {
    const ws = (d) => ({ launches: [a, b], default_launch: d });
    assert.equal(resolveDefaultLaunch(ws("b"), [cc], "builtin:claude-code"), b);
    assert.equal(resolveDefaultLaunch(ws("builtin:preset:task-intake"), [cc, orch], null), orch);
    // "shell" forza la shell vuota anche con default globale.
    assert.equal(resolveDefaultLaunch(ws(SHELL_LAUNCH_ID), [cc], "builtin:claude-code"), undefined);
    // Ereditato: null/assente => globale.
    assert.equal(resolveDefaultLaunch(ws(null), [cc], "builtin:claude-code"), cc);
    assert.equal(resolveDefaultLaunch({ launches: [a] }, [cc], "builtin:claude-code"), cc);
    // Id sparito => ripiega sul globale.
    assert.equal(resolveDefaultLaunch(ws("gone"), [cc], "builtin:claude-code"), cc);
    // Globale sconosciuto o assente => nessuno.
    assert.equal(resolveDefaultLaunch(ws(null), [cc], "builtin:nope"), undefined);
    assert.equal(resolveDefaultLaunch(ws(null), [cc], null), undefined);
    assert.equal(resolveDefaultLaunch({}, [], undefined), undefined);
  });

  it("launchCommands: integrato salva restore_command, custom uguale", () => {
    assert.deepEqual(launchCommands(cc), { command: "claude --resume", startCommand: "claude" });
    assert.deepEqual(launchCommands(a), { command: "ls", startCommand: "ls" });
    assert.deepEqual(launchCommands(undefined), { command: "", startCommand: "" });
  });
});
