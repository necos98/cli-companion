import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  debounceDelay,
  isSafeAgentSessionId,
  isSafeTabId,
  parseSession,
  restoreCommand,
  serializeSession,
} from "../src/lib/session.ts";

const tab = (tabId, workspaceId, extra = {}) => ({ tabId, workspaceId, command: "", ...extra });
const doc = (o) => JSON.stringify({ version: 1, ...o });

describe("sessione: serializzazione e lettura", () => {
  it("roundtrip", () => {
    const data = {
      activeWorkspaceId: "w2",
      groups: [
        { workspaceId: "w1", order: ["a", "b"], activeTabId: "b", maximizedTabId: "a" },
        { workspaceId: "w2", order: ["c"], activeTabId: "c", maximizedTabId: null },
      ],
      tabs: [
        tab("a", "w1", { command: "claude", launchId: "l1", agentId: "claude-code", agentSessionId: "s-1" }),
        tab("b", "w1", { title: "vim" }),
        tab("c", "w2"),
      ],
    };
    const parsed = parseSession(serializeSession(data), ["w1", "w2"]);
    assert.deepEqual(parsed, { version: 1, ...data });
  });

  it("tollera garbage e versioni sconosciute", () => {
    for (const raw of [null, undefined, "", "   ", "non json", "[]", "null", "42", '"x"', "{}", doc({ version: 2, tabs: [tab("a", "w")] }), JSON.stringify({ tabs: [tab("a", "w")] })]) {
      assert.equal(parseSession(raw, ["w"]), null, String(raw));
    }
    assert.equal(parseSession(doc({ tabs: "x", groups: 5 }), ["w"]), null);
    assert.equal(parseSession(doc({ tabs: [] }), ["w"]), null);
  });

  it("scarta tab con workspace inesistente, id invalidi o duplicati", () => {
    const raw = doc({
      activeWorkspaceId: "gone",
      groups: [
        { workspaceId: "gone", order: ["x"], activeTabId: "x" },
        { workspaceId: "w", order: ["a", "bad id", "a", "x"], activeTabId: "a" },
      ],
      tabs: [tab("x", "gone"), tab("a", "w"), tab("bad id", "w"), tab("../e", "w"), tab("a", "w", { command: "dup" }), 7, null, "s"],
    });
    const p = parseSession(raw, ["w"]);
    assert.deepEqual(p.tabs.map((t) => t.tabId), ["a"]);
    assert.equal(p.tabs[0].command, "");
    assert.deepEqual(p.groups, [{ workspaceId: "w", order: ["a"], activeTabId: "a", maximizedTabId: null }]);
    assert.equal(p.activeWorkspaceId, "w");
  });

  it("ripara gruppi incoerenti: attivo/massimizzato fuori ordine, tab orfani", () => {
    const raw = doc({
      activeWorkspaceId: "w2",
      groups: [{ workspaceId: "w1", order: ["a"], activeTabId: "zzz", maximizedTabId: "qqq" }],
      tabs: [tab("a", "w1"), tab("b", "w1"), tab("c", "w2")],
    });
    const p = parseSession(raw, ["w1", "w2"]);
    assert.deepEqual(p.groups[0], { workspaceId: "w1", order: ["a", "b"], activeTabId: "a", maximizedTabId: null });
    assert.deepEqual(p.groups[1], { workspaceId: "w2", order: ["c"], activeTabId: "c", maximizedTabId: null });
    assert.equal(p.activeWorkspaceId, "w2");
  });

  it("un tab listato nel gruppo di un altro workspace segue il proprio workspace", () => {
    const raw = doc({
      groups: [{ workspaceId: "w1", order: ["a", "b"], activeTabId: "a" }],
      tabs: [tab("a", "w1"), tab("b", "w2")],
    });
    const p = parseSession(raw, ["w1", "w2"]);
    assert.deepEqual(p.groups.map((g) => [g.workspaceId, g.order]), [["w1", ["a"]], ["w2", ["b"]]]);
  });

  it("ignora un agentSessionId non sicuro e tronca i campi lunghi", () => {
    const p = parseSession(
      doc({ tabs: [tab("a", "w", { agentSessionId: "x; rm -rf /", command: "c".repeat(5000), title: "t".repeat(500) })] }),
      ["w"],
    );
    assert.equal(p.tabs[0].agentSessionId, undefined);
    assert.equal(p.tabs[0].command.length, 1000);
    assert.equal(p.tabs[0].title.length, 80);
  });
});

describe("sessione: id sicuri", () => {
  it("tab id", () => {
    assert.ok(isSafeTabId("3f2a-4b_c"));
    for (const bad of ["", "a b", "a/b", "a.b", "x".repeat(65), 5, null]) assert.ok(!isSafeTabId(bad), String(bad));
  });
  it("agent session id", () => {
    assert.ok(isSafeAgentSessionId("0b9e8c1a-1234-4abc-9def-0123456789ab"));
    for (const bad of ["", "a b", "a;b", "$(x)", "a`b", "x".repeat(101), undefined]) assert.ok(!isSafeAgentSessionId(bad), String(bad));
  });
});

describe("restoreCommand", () => {
  const SID = "0b9e8c1a-1234-4abc-9def-0123456789ab";

  it("senza sessione agente: comando originale", () => {
    assert.equal(restoreCommand({ command: "" }), "");
    assert.equal(restoreCommand({ command: "  npm run dev " }), "npm run dev");
    assert.equal(restoreCommand({ command: "claude" }), "claude");
  });

  it("comando claude: aggiunge --resume", () => {
    assert.equal(restoreCommand({ command: "claude", agentSessionId: SID }), `claude --resume ${SID}`);
    assert.equal(
      restoreCommand({ command: "claude.exe --model opus", agentSessionId: SID }),
      `claude.exe --model opus --resume ${SID}`,
    );
    assert.equal(restoreCommand({ command: "CLAUDE", agentSessionId: SID }), `CLAUDE --resume ${SID}`);
  });

  it("toglie --resume/-r/--continue/-c esistenti", () => {
    const cases = [
      ["claude --resume old-id", "claude"],
      ["claude -r old-id --model opus", "claude --model opus"],
      ["claude --continue", "claude"],
      ["claude -c --model opus", "claude --model opus"],
      ["claude --resume=old", "claude"],
      ["claude --resume --model opus", "claude --model opus"],
      ["claude --model opus -c", "claude --model opus"],
      ["claude --append-system-prompt \"a b\" -r x", "claude --append-system-prompt \"a b\""],
    ];
    for (const [command, base] of cases) {
      assert.equal(restoreCommand({ command, agentSessionId: SID }), `${base} --resume ${SID}`, command);
    }
  });

  it("non tocca flag simili (--cwd, -cc)", () => {
    assert.equal(
      restoreCommand({ command: "claude --color auto", agentSessionId: SID }),
      `claude --color auto --resume ${SID}`,
    );
  });

  it("shell semplice o altro comando con sessione agente: claude --resume", () => {
    assert.equal(restoreCommand({ command: "", agentSessionId: SID }), `claude --resume ${SID}`);
    assert.equal(restoreCommand({ command: "npm run dev", agentSessionId: SID }), `claude --resume ${SID}`);
    assert.equal(restoreCommand({ command: "claudette", agentSessionId: SID }), `claude --resume ${SID}`);
    assert.equal(restoreCommand({ command: "x", agentId: "claude-code", agentSessionId: SID }), `claude --resume ${SID}`);
  });

  it("agentSessionId non sicuro: ignorato", () => {
    for (const bad of ["a; rm -rf /", "$(x)", "a b", ""]) {
      assert.equal(restoreCommand({ command: "claude -c", agentSessionId: bad }), "claude -c");
      assert.equal(restoreCommand({ command: "", agentSessionId: bad }), "");
    }
  });

  it("altro agente: mai claude --resume", () => {
    assert.equal(restoreCommand({ command: "codex", agentId: "codex", agentSessionId: SID }), "codex");
  });
});

describe("restoreCommand: comandi lunghi", () => {
  const SID = "0b9e8c1a-1234-4abc-9def-0123456789ab";
  it("--resume <id> non viene mai troncato", () => {
    const long = "claude --append-system-prompt " + "x".repeat(970);
    assert.equal(long.length, 1000);
    const r = restoreCommand({ command: long, agentSessionId: SID });
    assert.ok(r.length > 1000);
    assert.ok(r.endsWith(` --resume ${SID}`));
  });
  it("la base e' limitata a 1000 caratteri prima del resume", () => {
    const r = restoreCommand({ command: "claude " + "y".repeat(1500), agentSessionId: SID });
    assert.ok(r.endsWith(` --resume ${SID}`));
    assert.equal(r.length, 1000 + ` --resume ${SID}`.length);
  });
});

describe("debounceDelay", () => {
  it("rispetta l'attesa massima dalla prima modifica", () => {
    assert.equal(debounceDelay(1000, 1000, 500, 2000), 500);
    assert.equal(debounceDelay(2800, 1000, 500, 2000), 200);
    assert.equal(debounceDelay(3500, 1000, 500, 2000), 0);
  });
});
