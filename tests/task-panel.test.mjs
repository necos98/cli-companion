import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  activityGroups,
  activityTotals,
  dependencyLabels,
  dependsOnText,
  dropSlot,
  failureReason,
  groupSummary,
  moveBeforeId,
  proposalsText,
  queueSubtitle,
  runningMeta,
  specExcerpt,
  stripMarkdown,
} from "../src/lib/taskPanel.ts";

const task = (id, status, extra = {}) => ({
  id,
  title: `T ${id}`,
  task: "",
  depends_on: [],
  agent: "claude-code",
  status,
  attempts: 0,
  created_at: 0,
  updated_at: 0,
  waiting_on: [],
  attention: false,
  ...extra,
});

describe("stripMarkdown / specExcerpt", () => {
  it("toglie titoli, elenchi, enfasi, codice e link", () => {
    const md = "## Goal\n- usa **grassetto** e `codice`\n1. vedi [la doc](http://x.y)\n```rust\nfn a(){}\n```";
    assert.equal(stripMarkdown(md), "Goal usa grassetto e codice vedi la doc fn a(){}");
  });
  it("accorcia con i puntini", () => {
    const out = specExcerpt("a".repeat(300), 50);
    assert.equal(out.length, 50);
    assert.ok(out.endsWith("…"));
    assert.equal(specExcerpt("breve"), "breve");
  });
});

describe("dependencyLabels", () => {
  it("usa il titolo se noto, altrimenti l'id", () => {
    const labels = dependencyLabels(["a", "b", "c"], [task("a", "queued")], [{ id: "b", title: "Bee" }]);
    assert.deepEqual(labels, ["T a", "Bee", "c"]);
    assert.deepEqual(dependencyLabels(undefined, []), []);
  });
  it("dependsOnText", () => {
    assert.equal(dependsOnText([]), null);
    assert.equal(dependsOnText(["A", "B"]), "Dipende da A, B");
  });
});

describe("failureReason / runningMeta / queueSubtitle", () => {
  it("preferisce la nota, poi il riassunto, su una riga", () => {
    assert.equal(failureReason({ note: " stopped\nby the user ", report: { summary: "x" } }), "stopped by the user");
    assert.equal(failureReason({ note: null, report: { status: "failed", summary: "Test falliti" } }), "Test falliti");
    assert.equal(failureReason({ note: null, report: null }), "");
  });
  it("runningMeta: modello e tempo", () => {
    assert.equal(runningMeta({ model: "sonnet", started_at: 1000 }, 1000 + 12 * 60_000), "sonnet · 12m");
    assert.equal(runningMeta({ model: null, started_at: 1000 }, 31_000), "30s");
    assert.equal(runningMeta({ model: "opus", started_at: null }, 5), "opus");
  });
  it("queueSubtitle", () => {
    assert.equal(queueSubtitle({ model: "opus" }, "attende X"), "attende X");
    assert.equal(queueSubtitle({ model: "opus" }, null), "opus");
    assert.equal(queueSubtitle({ model: null }, null), "");
  });
});

describe("riordino", () => {
  const ids = ["a", "b", "c", "d"];
  it("dropSlot", () => {
    assert.equal(dropSlot(2, false), 2);
    assert.equal(dropSlot(2, true), 3);
  });
  it("sposta davanti a un elemento", () => {
    assert.equal(moveBeforeId(ids, "d", 1), "b");
    assert.equal(moveBeforeId(ids, "a", 3), "d");
  });
  it("in fondo = null", () => {
    assert.equal(moveBeforeId(ids, "a", 4), null);
  });
  it("stessa posizione o id ignoto = nessuna mossa", () => {
    assert.equal(moveBeforeId(ids, "b", 1), undefined);
    assert.equal(moveBeforeId(ids, "b", 2), undefined);
    assert.equal(moveBeforeId(ids, "d", 4), undefined);
    assert.equal(moveBeforeId(ids, "zz", 0), undefined);
  });
  it("limita lo slot all'intervallo", () => {
    assert.equal(moveBeforeId(ids, "c", -5), "a");
    assert.equal(moveBeforeId(ids, "a", 99), null);
  });
});

describe("vista Globale", () => {
  const ws = [{ id: "w1" }, { id: "w2" }, { id: "w3" }, { id: "w4" }];
  const boards = {
    w1: {
      tasks: [
        task("r1", "running"),
        task("r2", "running", { attention: true }),
        task("q1", "queued", { waiting_on: ["r1"] }),
        task("q2", "queued"),
        task("d", "done"),
      ],
      proposals: [],
    },
    w2: { tasks: [task("f", "failed")], proposals: [] },
    w3: { tasks: [task("d2", "done")], proposals: [] },
    // w4 senza board
  };
  it("include solo i workspace con attività", () => {
    const groups = activityGroups(ws, boards);
    assert.deepEqual(groups.map((g) => g.workspace.id), ["w1", "w2"]);
    assert.equal(groups[0].running.length, 2);
    assert.equal(groups[0].queued.length, 2);
    assert.equal(groups[0].next.id, "q2");
    assert.equal(groups[1].failed, 1);
    assert.equal(groups[1].next, null);
  });
  it("le sole proposte bastano", () => {
    const g = activityGroups([{ id: "x" }], { x: { tasks: [], proposals: [{ id: "p" }] } });
    assert.equal(g.length, 1);
    assert.equal(g[0].proposals, 1);
  });
  it("se tutti aspettano, il prossimo è il primo in coda", () => {
    const g = activityGroups([{ id: "x" }], {
      x: { tasks: [task("a", "queued", { waiting_on: ["z"] })], proposals: [] },
    });
    assert.equal(g[0].next.id, "a");
  });
  it("totali e riepilogo", () => {
    const groups = activityGroups(ws, boards);
    assert.deepEqual(activityTotals(groups), { running: 2, attention: 1, queued: 2, failed: 1, proposals: 0 });
    assert.equal(groupSummary(groups[0]), "2 in corso · 2 in coda");
    assert.equal(groupSummary(groups[1]), "1 fallito");
    assert.equal(groupSummary({ running: [], queued: [], failed: 3 }), "3 falliti");
  });
  it("proposalsText", () => {
    assert.equal(proposalsText(1), "1 proposta in attesa");
    assert.equal(proposalsText(3), "3 proposte in attesa");
  });
});
