import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { E2E_STATUS, afterRunResponse, e2eSummary } from "../src/lib/e2e.ts";

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
