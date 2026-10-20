import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  WORKFLOW_MAX,
  WORKFLOW_WIZARD_COMMAND,
  buildAutoOrchestrate,
  canSaveWorkflow,
  effectiveAutoOrchestrate,
  isAutoWorkspace,
  modelError,
  sameAutoOrchestrate,
  sanitizeModel,
  wizardAvailability,
  workflowStatusText,
} from "../src/lib/workflow.ts";

const env = (extra = {}) => ({
  agent_id: "claude-code",
  env_key: "windows",
  env_label: "Windows",
  found: true,
  installed: true,
  ...extra,
});

describe("workflow: orchestratore automatico", () => {
  it("modello: trim, vuoto = null, niente spazi interni", () => {
    assert.equal(sanitizeModel("  opus "), "opus");
    assert.equal(sanitizeModel("   "), null);
    assert.ok(modelError("a b"));
    assert.equal(modelError(" opus "), null);
    assert.equal(modelError("claude-opus-4.5_x"), null);
    assert.equal(modelError(""), null);
    assert.ok(modelError("a/b"));
    assert.ok(modelError("modèl"));
    assert.ok(modelError("x".repeat(65)));
    assert.equal(modelError("x".repeat(64)), null);
  });
  it("config inviata", () => {
    assert.deepEqual(buildAutoOrchestrate({ enabled: true, agent: "claude-code", model: " " }), {
      enabled: true,
      agent: "claude-code",
      model: null,
    });
  });
  it("config effettiva di default", () => {
    assert.deepEqual(effectiveAutoOrchestrate({}), {
      enabled: false,
      agent: "claude-code",
      model: null,
    });
    const ws = { auto_orchestrate: { enabled: true, agent: "x", model: "m" } };
    assert.equal(isAutoWorkspace(ws), true);
    assert.equal(isAutoWorkspace({ auto_orchestrate: null }), false);
    assert.equal(sameAutoOrchestrate(effectiveAutoOrchestrate(ws), ws.auto_orchestrate), true);
  });
});

describe("workflow: testo", () => {
  it("stato", () => {
    assert.equal(workflowStatusText(null), "Assente");
    assert.equal(workflowStatusText("  "), "Assente");
    assert.match(workflowStatusText("abc"), /Presente .* 3 caratteri/);
  });
  it("salvabile: non vuoto, entro il massimo, diverso dal salvato", () => {
    assert.equal(canSaveWorkflow("", null), false);
    assert.equal(canSaveWorkflow("x", null), true);
    assert.equal(canSaveWorkflow("x", "x"), false);
    assert.equal(canSaveWorkflow("x".repeat(WORKFLOW_MAX + 1), null), false);
    assert.equal(canSaveWorkflow("x".repeat(WORKFLOW_MAX), null), true);
  });
});

describe("workflow: wizard", () => {
  it("comando", () => {
    assert.equal(WORKFLOW_WIZARD_COMMAND, 'claude "/cli-companion-notify:workflow"');
  });
  it("verifica in corso con l'elenco vuoto", () => {
    const r = wizardAvailability({ kind: "windows" }, []);
    assert.equal(r.available, false);
    assert.ok(r.hint);
  });
  it("Windows installato", () => {
    assert.equal(wizardAvailability({ kind: "windows" }, [env()]).available, true);
  });
  it("WSL: serve la distro del workspace", () => {
    const envs = [env(), env({ env_key: "Ubuntu", env_label: "Ubuntu" })];
    assert.equal(wizardAvailability({ kind: "wsl", distro: "Ubuntu" }, envs).available, true);
    assert.equal(wizardAvailability({ kind: "wsl", distro: "Debian" }, envs).available, false);
  });
  it("non trovato o plugin non installato", () => {
    assert.match(
      wizardAvailability({ kind: "windows" }, [env({ found: false })]).hint,
      /non e' stato trovato/,
    );
    assert.match(
      wizardAvailability({ kind: "windows" }, [env({ installed: false })]).hint,
      /Installa il plugin/,
    );
  });
});
