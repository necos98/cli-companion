import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  CAPABILITY_LABELS,
  capabilityLabels,
  declaresAny,
  defaultOf,
  disableImpact,
  disableImpactLines,
  enabledEnvStatuses,
  enabledOf,
  envActions,
  envState,
  envStateText,
  onlyEnabled,
  removableEnvs,
} from "../src/lib/integrations.ts";

const status = (over = {}) => ({
  agent_id: "agent-a",
  agent_name: "Agent A",
  env_key: "windows",
  env_label: "Windows",
  env_kind: "windows",
  found: true,
  version: "2.1.211",
  installed: false,
  ...over,
});

const env = (readiness, over = {}) => ({ readiness, status: status(over) });

const info = (id, over = {}) => ({
  id,
  name: id.toUpperCase(),
  enabled: true,
  is_default: false,
  capabilities: [],
  models: [],
  effort_levels: [],
  default_model: null,
  envs: [],
  ...over,
});

describe("integrazioni: capability", () => {
  it("ogni contratto ha un'etichetta in italiano", () => {
    assert.equal(Object.keys(CAPABILITY_LABELS).length, 14);
    for (const label of Object.values(CAPABILITY_LABELS)) assert.ok(label.trim().length > 0);
  });

  it("etichette nell'ordine della tabella, senza doppioni", () => {
    assert.deepEqual(capabilityLabels(["plugin-skills", "automation", "session-resume", "plugin-skills"]), [
      "Ripresa sessioni",
      "Worker",
      "Skill del plugin",
    ]);
    assert.deepEqual(capabilityLabels([]), []);
  });

  it("declaresAny guarda tutte le integrazioni date", () => {
    const list = [info("a", { capabilities: ["models"] }), info("b", { capabilities: ["usage-limits"] })];
    assert.equal(declaresAny(list, ["usage-limits", "session-usage"]), true);
    assert.equal(declaresAny([list[0]], ["usage-limits", "session-usage"]), false);
    assert.equal(declaresAny([], ["usage-limits"]), false);
  });
});

describe("integrazioni: stato per ambiente", () => {
  it("prontezza -> stato e testo", () => {
    assert.equal(envState(env("ready", { installed: true })), "ready");
    assert.equal(envState(env("outdated", { installed: true })), "outdated");
    assert.equal(envState(env("not-installed")), "found");
    assert.equal(envState(env("not-found", { found: false })), "not-found");
    assert.equal(envStateText(env("ready")), "Pronto · v2.1.211");
    assert.equal(envStateText(env("outdated")), "Da aggiornare · v2.1.211");
    assert.equal(envStateText(env("not-installed", { version: null })), "Trovato");
    assert.equal(envStateText(env("not-found", { found: false, version: "1.0" })), "Non rilevato");
  });

  it("integrazione disabilitata: si legge lo stato grezzo dell'ambiente", () => {
    assert.equal(envState(env("disabled", { found: false })), "not-found");
    assert.equal(envState(env("disabled")), "found");
    assert.equal(envState(env("disabled", { installed: true })), "installed");
    assert.equal(envStateText(env("disabled", { installed: true, version: " 3.0 " })), "Installato · v3.0");
  });

  it("azioni: installa, aggiorna o rimuovi secondo lo stato", () => {
    assert.deepEqual(envActions(env("not-installed"), true), ["install"]);
    assert.deepEqual(envActions(env("outdated"), true), ["update", "remove"]);
    assert.deepEqual(envActions(env("ready"), true), ["remove"]);
    assert.deepEqual(envActions(env("not-found", { found: false }), true), []);
  });

  it("integrazione disabilitata: nessuna azione per ambiente", () => {
    for (const readiness of ["ready", "outdated", "not-installed", "disabled"]) {
      assert.deepEqual(envActions(env(readiness, { installed: true }), false), []);
    }
  });

  it("removableEnvs: solo agente trovato e pacchetto presente", () => {
    const present = env("disabled", { installed: true });
    const list = info("a", {
      enabled: false,
      envs: [
        present,
        env("disabled", { env_key: "Ubuntu", installed: false }),
        env("disabled", { env_key: "Debian", found: false, installed: true }),
      ],
    });
    assert.deepEqual(removableEnvs(list), [present]);
    assert.deepEqual(removableEnvs(info("b")), []);
  });
});

describe("integrazioni: abilitate e predefinita", () => {
  const a = info("a", { is_default: true, envs: [env("ready", { agent_id: "a", installed: true })] });
  const b = info("b", { enabled: false, envs: [env("disabled", { agent_id: "b", installed: true })] });
  const c = info("c", { envs: [env("not-installed", { agent_id: "c" })] });
  const list = [a, b, c];

  it("enabledOf filtra e mantiene il riferimento finché l'elenco è lo stesso", () => {
    assert.deepEqual(enabledOf(list), [a, c]);
    assert.equal(enabledOf(list), enabledOf(list));
    assert.notEqual(enabledOf([...list]), enabledOf(list));
    assert.deepEqual(enabledOf([]), []);
  });

  it("defaultOf: la predefinita abilitata, altrimenti null", () => {
    assert.equal(defaultOf(list), a);
    assert.equal(defaultOf([b, c]), null);
    assert.equal(defaultOf([info("x", { enabled: false, is_default: true })]), null);
    assert.equal(defaultOf([]), null);
  });

  it("enabledEnvStatuses: solo gli ambienti delle abilitate, stabile", () => {
    const statuses = enabledEnvStatuses(list);
    assert.deepEqual(
      statuses.map((s) => s.agent_id),
      ["a", "c"],
    );
    assert.equal(enabledEnvStatuses(list), statuses);
  });

  it("onlyEnabled filtra per agent_id", () => {
    const profiles = [{ agent_id: "a" }, { agent_id: "b" }, { agent_id: "z" }];
    assert.deepEqual(onlyEnabled(profiles, list), [{ agent_id: "a" }]);
  });
});

describe("integrazioni: impatto della disabilitazione", () => {
  const task = (agent, taskStatus) => ({ agent, status: taskStatus });

  it("conta terminali e task in coda dell'agente", () => {
    const impact = disableImpact(
      "a",
      ["a", null, "b", "a"],
      [task("a", "queued"), task("a", "running"), task("a", "done"), task("b", "queued"), task("a", "queued")],
    );
    assert.deepEqual(impact, { tabs: 2, queuedTasks: 2 });
  });

  it("nulla da contare", () => {
    assert.deepEqual(disableImpact("a", [], []), { tabs: 0, queuedTasks: 0 });
    assert.deepEqual(disableImpact("a", [null, "b"], [task("b", "queued")]), { tabs: 0, queuedTasks: 0 });
  });

  it("testo della conferma: singolare, plurale e assenza", () => {
    assert.deepEqual(disableImpactLines("Agent A", { tabs: 0, queuedTasks: 0 }), []);
    const [tabs] = disableImpactLines("Agent A", { tabs: 1, queuedTasks: 0 });
    assert.match(tabs, /^1 terminale aperto con Agent A/);
    const lines = disableImpactLines("Agent A", { tabs: 3, queuedTasks: 2 });
    assert.equal(lines.length, 2);
    assert.match(lines[0], /^3 terminali aperti con Agent A/);
    assert.match(lines[1], /^2 task in coda con Agent A: si fermano con un avviso/);
  });
});
