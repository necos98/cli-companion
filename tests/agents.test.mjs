import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  agentEnvBadge,
  agentEnvState,
  agentEnvStateText,
  agentInstallLabel,
  agentRowKey,
  applyAgentEvent,
  agentIdForCommand,
  applyUserInput,
  resolveInputOverride,
  tabAgentId,
  groupByAgent,
  installedAgentsSummary,
  countNeedingInput,
  terminalCountText,
  isTerminalReply,
  notificationBody,
  shouldSystemNotify,
  soundForEvent,
  tabStatus,
} from "../src/lib/agents.ts";
import { DING_SEQUENCE, ATTENTION_SEQUENCE } from "../src/lib/notifySound.ts";

const env = (over = {}) => ({
  agent_id: "claude-code",
  agent_name: "Claude Code",
  env_key: "windows",
  env_label: "Windows",
  env_kind: "windows",
  found: true,
  version: "2.1.211",
  installed: false,
  ...over,
});

describe("agenti: stato per ambiente", () => {
  it("stato e testo", () => {
    assert.equal(agentEnvState(env()), "not-installed");
    assert.equal(agentEnvState(env({ installed: true })), "installed");
    assert.equal(agentEnvState(env({ found: false })), "not-found");
    assert.equal(agentEnvStateText(env()), "Non installato · v2.1.211");
    assert.equal(agentEnvStateText(env({ installed: true })), "Installato · v2.1.211");
    assert.equal(agentEnvStateText(env({ version: null })), "Non installato");
    assert.equal(agentEnvStateText(env({ found: false, version: null })), "Non trovato");
  });

  it("etichetta del bottone e badge", () => {
    assert.equal(agentInstallLabel(env()), "Installa");
    assert.equal(agentInstallLabel(env({ installed: true })), "Reinstalla");
    assert.equal(agentInstallLabel(env({ found: false })), null);
    assert.equal(agentEnvBadge(env()), "WIN");
    assert.equal(
      agentEnvBadge(env({ env_kind: "wsl", env_label: "Ubuntu-24.04" })),
      "Ubuntu-24.04",
    );
  });

  it("raggruppa per agente mantenendo l'ordine", () => {
    const rows = [
      env(),
      env({ agent_id: "altro", agent_name: "Altro", env_key: "windows" }),
      env({ env_key: "Ubuntu", env_label: "Ubuntu", env_kind: "wsl" }),
    ];
    const groups = groupByAgent(rows);
    assert.deepEqual(
      groups.map((g) => [g.agentId, g.agentName, g.envs.length]),
      [
        ["claude-code", "Claude Code", 2],
        ["altro", "Altro", 1],
      ],
    );
    assert.deepEqual(groupByAgent([]), []);
    assert.equal(agentRowKey(rows[2]), "claude-code::Ubuntu");
  });
});

describe("agenti: macchina a stati del tab", () => {
  it("session-start marca il tab come agente e salva l'id sessione", () => {
    const s = applyAgentEvent(undefined, { event: "session-start", agent_session_id: "abc" });
    assert.deepEqual(s, { phase: "idle", agentSessionId: "abc" });
  });

  it("finished / needs-input aggiornano la fase e conservano l'id", () => {
    let s = applyAgentEvent(undefined, { event: "session-start", agent_session_id: "abc" });
    s = applyAgentEvent(s, { event: "finished" });
    assert.deepEqual(s, { phase: "finished", agentSessionId: "abc" });
    s = applyAgentEvent(s, { event: "needs-input", agent_session_id: "xyz" });
    assert.deepEqual(s, { phase: "needs-input", agentSessionId: "xyz" });
  });

  it("transcript_path: aggiornato se presente, conservato altrimenti (anche dopo session-end)", () => {
    let s = applyAgentEvent(undefined, { event: "session-start", transcript_path: "/a.jsonl" });
    assert.equal(s.transcriptPath, "/a.jsonl");
    s = applyAgentEvent(s, { event: "finished" });
    assert.equal(s.transcriptPath, "/a.jsonl");
    s = applyAgentEvent(s, { event: "needs-input", transcript_path: "/b.jsonl" });
    assert.equal(s.transcriptPath, "/b.jsonl");
    s = applyAgentEvent(s, { event: "session-end" });
    assert.equal(s.transcriptPath, "/b.jsonl");
  });

  it("finished senza session-start precedente rende comunque il tab un agente", () => {
    assert.deepEqual(applyAgentEvent(undefined, { event: "finished" }), { phase: "finished" });
  });

  it("eventi sconosciuti o test non cambiano lo stato", () => {
    assert.equal(applyAgentEvent(undefined, { event: "test" }), undefined);
    const prev = { phase: "running" };
    assert.equal(applyAgentEvent(prev, { event: "boh" }), prev);
  });

  it("Invio => running; altro input toglie check/alert; non-agente invariato", () => {
    assert.equal(applyUserInput(undefined, true), undefined);
    const idle = { phase: "idle", agentSessionId: "a" };
    assert.deepEqual(applyUserInput(idle, true), { phase: "running", agentSessionId: "a" });
    const running = { phase: "running" };
    assert.equal(applyUserInput(running, true), running);
    assert.equal(applyUserInput(running, false), running);
    assert.equal(applyUserInput(idle, false), idle);
    assert.deepEqual(applyUserInput({ phase: "finished" }, false), { phase: "idle" });
    assert.deepEqual(applyUserInput({ phase: "needs-input" }, false), { phase: "idle" });
    assert.deepEqual(applyUserInput({ phase: "finished" }, true), { phase: "running" });
  });

  it("riconosce le risposte automatiche del terminale", () => {
    for (const reply of ["\x1b[I", "\x1b[O", "\x1b[12;40R", "\x1b[?62;4c", "\x1b[>0;276;0c", "\x1b]11;rgb:0000/0000/0000\x07"]) {
      assert.equal(isTerminalReply(reply), true, JSON.stringify(reply));
    }
    for (const key of ["a", "\r", "\x1b", "\x1b[A", "\x1b[3~", "\x03", "ciao"]) {
      assert.equal(isTerminalReply(key), false, JSON.stringify(key));
    }
  });
});

describe("agenti: stato visibile nel tab", () => {
  it("tab normale: euristica busy/idle", () => {
    assert.deepEqual(tabStatus(undefined, true), { icon: "dot", tone: "busy", label: "In esecuzione" });
    assert.deepEqual(tabStatus(undefined, false), { icon: "dot", tone: "idle", label: "Inattivo" });
  });

  it("tab agente: la fase sostituisce l'euristica", () => {
    assert.equal(tabStatus({ phase: "running" }, false).tone, "busy");
    assert.equal(tabStatus({ phase: "idle" }, true).tone, "idle");
    assert.equal(tabStatus({ phase: "finished" }, true).icon, "check");
    assert.equal(tabStatus({ phase: "needs-input" }, false).icon, "alert");
    assert.equal(tabStatus({ phase: "needs-input" }, false).tone, "attention");
  });
});

describe("agenti: avvisi", () => {
  it("suoni per evento", () => {
    assert.equal(soundForEvent("finished"), "ding");
    assert.equal(soundForEvent("test"), "ding");
    assert.equal(soundForEvent("needs-input"), "attention");
    assert.equal(soundForEvent("session-start"), null);
  });

  it("sequenze: l'attenzione e' distinta e piu' acuta del ding", () => {
    assert.notDeepEqual(ATTENTION_SEQUENCE, DING_SEQUENCE);
    const max = (seq) => Math.max(...seq.map((n) => n.freq));
    assert.ok(max(ATTENTION_SEQUENCE) > max(DING_SEQUENCE));
    const end = ATTENTION_SEQUENCE.at(-1);
    assert.ok(end.at + end.dur < 0.6);
  });

  it("notifica di sistema solo per finished e needs-input", () => {
    assert.equal(shouldSystemNotify("finished"), true);
    assert.equal(shouldSystemNotify("needs-input"), true);
    assert.equal(shouldSystemNotify("session-start"), false);
    assert.equal(shouldSystemNotify("test"), false);
  });

  it("testo della notifica", () => {
    assert.equal(notificationBody("finished", "Dev"), "Dev: ha finito");
    assert.equal(notificationBody("needs-input", "Dev"), "Dev: aspetta una risposta");
    assert.equal(notificationBody("needs-input", "Dev", "  "), "Dev: aspetta una risposta");
    assert.equal(
      notificationBody("needs-input", "Dev", "Claude needs your permission"),
      "Dev: Claude needs your permission",
    );
  });
});

describe("agenti: id del provider nello stato del tab", () => {
  it("si ricorda l'agente dall'evento e lo conserva", () => {
    let s = applyAgentEvent(undefined, { event: "session-start", agent: "claude-code", agent_session_id: "abc" });
    assert.deepEqual(s, { phase: "idle", agentSessionId: "abc", agentId: "claude-code" });
    s = applyAgentEvent(s, { event: "finished" });
    assert.deepEqual(s, { phase: "finished", agentSessionId: "abc", agentId: "claude-code" });
    assert.deepEqual(applyUserInput(s, true), { phase: "running", agentSessionId: "abc", agentId: "claude-code" });
  });
});

describe("session-end", () => {
  it("riporta il tab a non-agente conservando l'id di sessione", () => {
    let s = applyAgentEvent(undefined, { event: "session-start", agent_session_id: "abc", agent: "claude-code" });
    s = applyAgentEvent(s, { event: "session-end" });
    assert.deepEqual(s, { phase: "idle", agentSessionId: "abc", agentId: "claude-code", ended: true });
    assert.deepEqual(tabStatus(s, true), tabStatus(undefined, true));
    assert.deepEqual(tabStatus(s, false), tabStatus(undefined, false));
    assert.equal(applyUserInput(s, true), s);
  });

  it("senza stato precedente: invariato; un nuovo session-start riattiva l'agente", () => {
    assert.equal(applyAgentEvent(undefined, { event: "session-end" }), undefined);
    const ended = { phase: "idle", agentSessionId: "abc", ended: true };
    const s = applyAgentEvent(ended, { event: "session-start", agent_session_id: "def" });
    assert.deepEqual(s, { phase: "idle", agentSessionId: "def" });
  });
});

describe("profili di input", () => {
  const profiles = [
    {
      agent_id: "claude-code",
      input_profile: {
        rules: [
          { event: "image-paste", env_kind: "windows", send: "\x1bv" },
          { event: "image-paste", env_kind: "wsl", send: "\x1bv" },
        ],
      },
    },
    {
      agent_id: "any",
      input_profile: { rules: [{ event: "image-paste", env_kind: null, send: "X" }] },
    },
    { agent_id: "empty", input_profile: { rules: [] } },
  ];

  it("resolveInputOverride: regola per agente e ambiente", () => {
    assert.equal(resolveInputOverride(profiles, "claude-code", "windows", "image-paste"), "\x1bv");
    assert.equal(resolveInputOverride(profiles, "claude-code", "wsl", "image-paste"), "\x1bv");
    assert.equal(resolveInputOverride(profiles, "claude-code", "mac", "image-paste"), null);
  });

  it("resolveInputOverride: env_kind null vale per tutti", () => {
    assert.equal(resolveInputOverride(profiles, "any", "wsl", "image-paste"), "X");
  });

  it("resolveInputOverride: agente ignoto, assente o senza regole", () => {
    assert.equal(resolveInputOverride(profiles, "nope", "windows", "image-paste"), null);
    assert.equal(resolveInputOverride(profiles, "empty", "windows", "image-paste"), null);
    assert.equal(resolveInputOverride(profiles, null, "windows", "image-paste"), null);
    assert.equal(resolveInputOverride(profiles, undefined, "windows", "image-paste"), null);
    assert.equal(resolveInputOverride([], "claude-code", "windows", "image-paste"), null);
  });

  it("agentIdForCommand: solo claude / claude.exe", () => {
    assert.equal(agentIdForCommand("claude"), "claude-code");
    assert.equal(agentIdForCommand("  Claude.exe --resume abc"), "claude-code");
    assert.equal(agentIdForCommand("claude-foo"), null);
    assert.equal(agentIdForCommand("npm run dev"), null);
    assert.equal(agentIdForCommand(""), null);
    assert.equal(agentIdForCommand(undefined), null);
  });

  it("tabAgentId: plugin > comando; finito = null", () => {
    assert.equal(tabAgentId({ agent: { agentId: "claude-code" }, startCommand: "" }), "claude-code");
    assert.equal(tabAgentId({ startCommand: "claude --resume x" }), "claude-code");
    assert.equal(tabAgentId({ startCommand: "" }), null);
    assert.equal(tabAgentId({}), null);
    assert.equal(
      tabAgentId({ agent: { agentId: "claude-code", ended: true }, startCommand: "claude" }),
      null,
    );
    // Stato agente senza id (evento senza agente): ripiega sul comando.
    assert.equal(tabAgentId({ agent: {}, startCommand: "claude" }), "claude-code");
  });
});

describe("barra di stato", () => {
  const env = (agent_id, agent_name, env_label, found, installed) => ({
    agent_id,
    agent_name,
    env_label,
    found,
    installed,
  });

  it("installedAgentsSummary: solo gli env installati, raggruppati per agente", () => {
    assert.equal(
      installedAgentsSummary([
        env("claude-code", "Claude Code", "Windows", true, true),
        env("claude-code", "Claude Code", "Ubuntu", true, false),
        env("claude-code", "Claude Code", "Debian", false, true),
        env("claude-code", "Claude Code", "Ubuntu-24.04", true, true),
      ]),
      "Claude Code · Windows, Ubuntu-24.04",
    );
    assert.equal(
      installedAgentsSummary([
        env("a", "A", "Windows", true, true),
        env("b", "B", "Windows", true, true),
      ]),
      "A · Windows; B · Windows",
    );
  });

  it("installedAgentsSummary: niente installato = stringa vuota", () => {
    assert.equal(installedAgentsSummary([]), "");
    assert.equal(installedAgentsSummary([env("a", "A", "Windows", true, false)]), "");
  });

  it("countNeedingInput e terminalCountText", () => {
    assert.equal(
      countNeedingInput([{ icon: "alert" }, { icon: "dot" }, { icon: "alert" }, { icon: "check" }]),
      2,
    );
    assert.equal(countNeedingInput([]), 0);
    assert.equal(terminalCountText(0), "0 terminali");
    assert.equal(terminalCountText(1), "1 terminale");
    assert.equal(terminalCountText(3), "3 terminali");
  });
});
