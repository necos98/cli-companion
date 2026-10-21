import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  diffLines,
  diffLists,
  formatScalar,
  pendingCount,
  repoChanges,
  resolutionLabel,
  splitRequests,
} from "../src/lib/requests.ts";

describe("requests", () => {
  it("diffLists marca rimossi, invariati e aggiunti", () => {
    assert.deepEqual(diffLists(["a", "b"], ["b", "c"]), [
      { value: "a", mark: "removed" },
      { value: "b", mark: "same" },
      { value: "c", mark: "added" },
    ]);
    assert.deepEqual(diffLists(undefined, ["x"]), [{ value: "x", mark: "added" }]);
  });

  it("diffLines produce un diff per righe", () => {
    assert.deepEqual(diffLines("a\nb\nc", "a\nx\nc"), [
      { text: "a", mark: "same" },
      { text: "b", mark: "removed" },
      { text: "x", mark: "added" },
      { text: "c", mark: "same" },
    ]);
    assert.deepEqual(diffLines("", "r1"), [{ text: "r1", mark: "added" }]);
    assert.deepEqual(diffLines("r1", ""), [{ text: "r1", mark: "removed" }]);
  });

  it("formatScalar", () => {
    assert.equal(formatScalar(null), "(non impostato)");
    assert.equal(formatScalar(true), "sì");
    assert.equal(formatScalar("main"), "main");
  });

  it("repoChanges usa etichette italiane e ordine fisso", () => {
    const ch = repoChanges(
      { repos: { ".": { base_branch: "dev", setup: ["npm i"] } } },
      { repos: { ".": { base_branch: "main", setup: [] } } },
    );
    assert.equal(ch.length, 1);
    assert.equal(ch[0].label, "(root)");
    assert.deepEqual(
      ch[0].fields.map((f) => f.label),
      ["Comandi di setup", "Branch base"],
    );
    assert.equal(ch[0].fields[1].before, "main");
    assert.equal(ch[0].fields[1].after, "dev");
  });

  it("splitRequests e pendingCount", () => {
    const reqs = [
      { id: "b", kind: "question", status: "pending", created_at: 2 },
      { id: "a", kind: "question", status: "pending", created_at: 1 },
      { id: "c", kind: "settings", status: "approved", created_at: 0, resolved_at: 5 },
    ];
    const s = splitRequests(reqs);
    assert.deepEqual(s.pending.map((r) => r.id), ["a", "b"]);
    assert.deepEqual(s.resolved.map((r) => r.id), ["c"]);
    assert.equal(pendingCount({ requests: reqs }), 2);
    assert.equal(pendingCount({ pending_requests: 4 }), 4);
    assert.equal(pendingCount(null), 0);
    assert.equal(resolutionLabel({ status: "rejected", kind: "settings" }), "Rifiutata");
  });
});
