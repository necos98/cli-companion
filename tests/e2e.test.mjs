import assert from "node:assert/strict";
import { describe, it } from "node:test";

import {
  E2E_STATUS,
  afterRunResponse,
  e2eBadge,
  e2eOption,
  e2eRunBadge,
  e2eSummary,
  repoHasE2e,
  specE2eBadge,
  taskE2eRun,
  withRepoE2e,
} from "../src/lib/e2e.ts";

const run = (over = {}) => ({ run_id: "r", status: "passed", started_at: 1, junit: "j.xml", ...over });

describe("E2E_STATUS", () => {
  it("etichetta ogni stato in italiano", () => {
    assert.deepEqual(
      Object.values(E2E_STATUS).map((s) => s.label),
      ["in corso", "superati", "falliti", "errore"],
    );
  });
});

describe("e2eSummary", () => {
  it("totali, durata e commit abbreviato", () => {
    const r = run({
      totals: { tests: 12, passed: 11, failed: 1, skipped: 0 },
      duration_ms: 83_000,
      code: { commit: "abcdef0123456789", tree: "t", changes: "c" },
    });
    assert.equal(e2eSummary(r), "12 test: 11 superati, 1 fallito, 0 saltati · 1m · abcdef0");
  });

  it("singolari e instabili", () => {
    const r = run({ totals: { tests: 1, passed: 1, failed: 0, skipped: 1, flaky: 1 } });
    assert.equal(e2eSummary(r), "1 test: 1 superato, 0 falliti, 1 saltato, 1 instabile");
  });

  it("senza dati e' vuota", () => {
    assert.equal(e2eSummary(run({ status: "running" })), "");
  });
});

describe("afterRunResponse", () => {
  it("la risposta si usa se non c'e' ancora uno stato", () => {
    const response = run({ status: "running" });
    assert.equal(afterRunResponse(null, response), response);
  });

  it("l'esecuzione gia' conclusa da un evento non torna 'in corso'", () => {
    const finished = run({ status: "error" });
    assert.equal(afterRunResponse(finished, run({ status: "running" })), finished);
  });

  it("una esecuzione diversa si sostituisce", () => {
    const response = run({ run_id: "r2", status: "running" });
    assert.equal(afterRunResponse(run(), response), response);
  });
});

describe("e2eRunBadge", () => {
  it("superati: passati/totali, verde", () => {
    const b = e2eRunBadge(run({ totals: { tests: 12, passed: 12, failed: 0, skipped: 0 } }));
    assert.equal(b.label, "E2E 12/12");
    assert.equal(b.className, "text-status-finished");
  });

  it("falliti: numero dei falliti, rosso", () => {
    const two = e2eRunBadge(run({ status: "failed", totals: { tests: 12, passed: 10, failed: 2, skipped: 0 } }));
    assert.equal(two.label, "E2E 2 falliti");
    assert.equal(two.className, "text-destructive");
    const one = e2eRunBadge(run({ status: "failed", totals: { tests: 3, passed: 2, failed: 1, skipped: 0 } }));
    assert.equal(one.label, "E2E 1 fallito");
  });

  it("senza totali resta generico", () => {
    assert.equal(e2eRunBadge(run()).label, "E2E superati");
    assert.equal(e2eRunBadge(run({ status: "failed" })).label, "E2E falliti");
  });

  it("in corso ed errore", () => {
    assert.equal(e2eRunBadge(run({ status: "running" })).label, "E2E in corso");
    assert.equal(e2eRunBadge(run({ status: "error" })).label, "E2E errore");
  });
});

describe("e2eBadge", () => {
  it("niente se il task non e' marcato e non ha esecuzioni", () => {
    assert.equal(e2eBadge({}), null);
    assert.equal(e2eBadge({ e2e: false }), null);
  });

  it("marcato senza esecuzioni: richiesto", () => {
    assert.equal(e2eBadge({ e2e: true }).label, "E2E richiesto");
  });

  it("l'esecuzione del task vince su quella del report", () => {
    const task = { e2e: true, e2e_run: run({ status: "running" }), report: { e2e: run() } };
    assert.equal(e2eBadge(task).label, "E2E in corso");
  });

  it("senza esecuzione sul task si usa il report", () => {
    const task = { e2e: true, e2e_run: null, report: { e2e: run({ status: "error" }) } };
    assert.equal(taskE2eRun(task).status, "error");
    assert.equal(e2eBadge(task).label, "E2E errore");
  });

  it("anche un task non marcato mostra l'esito (es. review)", () => {
    assert.equal(e2eBadge({ report: { e2e: run() } }).label, "E2E superati");
  });
});

describe("specE2eBadge", () => {
  it("solo il flag", () => {
    assert.equal(specE2eBadge({ e2e: true }).label, "E2E richiesto");
    assert.equal(specE2eBadge({}), null);
  });
});

describe("repoHasE2e", () => {
  const repos = { ".": { e2e: { command: "x" } }, app: {}, lib: { e2e: { command: "y" } } };
  it("legge la configurazione del repository, la root e' '.'", () => {
    assert.equal(repoHasE2e(repos, "."), true);
    assert.equal(repoHasE2e(repos, ""), true);
    assert.equal(repoHasE2e(repos, null), true);
    assert.equal(repoHasE2e(repos, "lib"), true);
    assert.equal(repoHasE2e(repos, "app"), false);
    assert.equal(repoHasE2e(repos, "altro"), false);
    assert.equal(repoHasE2e(undefined, "."), false);
  });
});

describe("e2eOption", () => {
  it("configurato: attiva", () => {
    const o = e2eOption(false, true);
    assert.equal(o.checked, false);
    assert.equal(o.disabled, false);
  });

  it("non configurato: disattivata, con il rimando alle impostazioni", () => {
    const o = e2eOption(false, false);
    assert.equal(o.disabled, true);
    assert.match(o.help, /Impostazioni/);
  });

  it("flag gia' acceso senza configurazione: si puo' togliere", () => {
    const o = e2eOption(true, false);
    assert.equal(o.checked, true);
    assert.equal(o.disabled, false);
  });
});

describe("withRepoE2e", () => {
  const repos = { ".": { e2e: {} }, app: {} };
  it("repo senza E2E spegne il flag", () => {
    assert.deepEqual(withRepoE2e({ repo: "app" }, repos), { repo: "app", e2e: false });
  });
  it("repo con E2E o patch senza repo non cambia", () => {
    const keep = { repo: ".", e2e: true };
    assert.equal(withRepoE2e(keep, repos), keep);
    const other = { title: "t" };
    assert.equal(withRepoE2e(other, repos), other);
  });
});
