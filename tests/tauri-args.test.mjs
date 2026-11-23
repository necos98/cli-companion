import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const RS = path.join(root, "src-tauri", "src");

// Tipi iniettati da Tauri (non arrivano dal JSON): esclusi dal confronto.
// `Request` porta il corpo raw e gli header della chiamata (comandi con payload binario).
const INJECTED = new Set(["AppHandle", "App", "Window", "WebviewWindow", "Webview", "State", "EventId", "Request"]);

const toCamel = (s) => s.replace(/_([a-z])/g, (_, c) => c.toUpperCase());

function rustCommands() {
  const out = new Map();
  for (const file of [
    "pty/mod.rs",
    "workspaces/commands.rs",
    "workspaces/launches.rs",
    "workspaces/explorer.rs",
    "workspaces/location.rs",
    "wsl.rs",
    "updater.rs",
    "ipc/server.rs",
    "toast.rs",
    "agents/commands.rs",
    "agents/integrations_commands.rs",
    "session.rs",
    "automation/commands.rs",
    "automation/auto.rs",
    "launch_settings.rs",
    "bots.rs",
    "bots/commands.rs",
    "hub/settings/commands.rs",
    "hub/commands.rs",
    "hub/action_requests.rs",
    "hub/thread_state.rs",
    "hub/thread_delete.rs",
    "hub/attachments/commands.rs",
    "hub/engine/status.rs",
    "connectors/commands/instances.rs",
    "connectors/commands/actions.rs",
    "connectors/commands/routes.rs",
    "store/warnings.rs",
    "headroom/commands.rs",
  ]) {
    const src = readFileSync(path.join(RS, file), "utf8");
    const re = /#\[tauri::command\]\s*pub (?:async )?fn (\w+)\s*\(([^)]*)\)/g;
    for (const m of src.matchAll(re)) {
      const args = [];
      for (const param of m[2].split(",")) {
        const pm = param.trim().match(/^(\w+)\s*:\s*(.+)$/);
        if (!pm) continue;
        const base = pm[2].trim().split("<")[0].split("::").pop();
        if (INJECTED.has(base)) continue;
        args.push(toCamel(pm[1]));
      }
      out.set(m[1], args.sort());
    }
  }
  return out;
}

function frontendInvokes() {
  const src = readFileSync(path.join(root, "src", "lib", "tauri.ts"), "utf8");
  const out = new Map();
  const re = /invoke<[^>]*>\(\s*"([^"]+)"\s*(?:,\s*\{([^}]*)\})?\)/g;
  for (const m of src.matchAll(re)) {
    const keys = [];
    if (m[2] !== undefined) {
      // Supporta sia { a, b } (shorthand) che { a: x, b: y }.
      for (const raw of m[2].split(",")) {
        const chunk = raw.trim();
        const em = chunk.match(/^(\w+)\s*:/);
        if (em) keys.push(em[1]);
        else if (/^\w+$/.test(chunk)) keys.push(chunk);
      }
    }
    out.set(m[1], keys.sort());
  }
  // Payload binario: `invoke<T>("nome", byte, { headers })`, nessun argomento con nome.
  for (const m of src.matchAll(/invoke<[^>]*>\(\s*"([^"]+)"\s*,\s*\w+\s*,/g)) out.set(m[1], []);
  return out;
}

// Regressione "pty_spawn missing required key workspaceId": Tauri v2 espone
// gli argomenti dei comandi in camelCase. Il sorgente Rust fa fede: questo
// test fallisce se il frontend invia chiavi diverse (snake_case in più/meno).
describe("contratto invoke frontend <-> comandi Rust", () => {
  it("ogni comando registrato ha gli stessi argomenti nel frontend (camelCase)", () => {
    const libRs = readFileSync(path.join(RS, "lib.rs"), "utf8");
    const handler = libRs.match(/generate_handler!\s*\[([\s\S]*?)\]/);
    assert.ok(handler, "generate_handler! non trovato in lib.rs");
    const registered = [...handler[1].matchAll(/(?:\w+::)+(\w+)/g)].map((m) => m[1]);

    const rust = rustCommands();
    const js = frontendInvokes();
    assert.ok(rust.size > 0 && js.size > 0, "parse fallita, test non significativo");

    const problems = [];
    for (const cmd of registered) {
      if (!rust.has(cmd)) problems.push(`${cmd}: registrato ma #[tauri::command] non trovato`);
      else if (!js.has(cmd)) problems.push(`${cmd}: registrato ma mai invocato dal frontend`);
      else {
        const want = rust.get(cmd).join(",");
        const got = js.get(cmd).join(",");
        if (want !== got) problems.push(`${cmd}: rust vuole {${want}} ma il frontend invia {${got}}`);
      }
    }
    assert.deepEqual(problems, [], problems.join("\n"));
  });
});
