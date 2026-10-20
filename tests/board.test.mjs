import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  ACTION_LABELS,
  COLUMNS,
  activeCount,
  allowedActions,
  canEditModel,
  canOpenTerminal,
  clampMaxParallel,
  columnCounts,
  columnLabel,
  columnTextClass,
  emptyBoard,
  isBoardOf,
  mergeBoard,
  sanitizeWorktreeSetup,
  taskIdsKey,
  WORKTREE_SETUP_MAX,
  progress,
  shortReport,
  workerSubStatus,
} from "../src/lib/board.ts";
import { boardEdges, layoutBoard, layoutSignature, NODE_WIDTH } from "../src/lib/boardLayout.ts";

const task = (extra = {}) => ({
  id: "a",
  status: "queued",
  column: "ready",
  held: false,
  depends_on: [],
  ...extra,
});

describe("board: colonne", () => {
  it("etichette italiane del contratto", () => {
    assert.deepEqual(COLUMNS.map(columnLabel), [
      "In attesa",
      "Pronto",
      "In corso",
      "Da integrare",
      "Fatto",
      "Fallito",
      "Bloccato",
      "Annullato",
    ]);
  });
  it("colonna sconosciuta: etichetta invariata e colore neutro", () => {
    assert.equal(columnLabel("boh"), "boh");
    assert.equal(columnTextClass("boh"), "text-muted-foreground");
  });
  it("conteggio per colonna", () => {
    const c = columnCounts([{ column: "done" }, { column: "done" }, { column: "failed" }]);
    assert.equal(c.done, 2);
    assert.equal(c.failed, 1);
    assert.equal(c.running, 0);
  });
});

describe("board: azioni consentite", () => {
  it("queued: trattieni / rilascia e annulla", () => {
    assert.deepEqual(allowedActions(task()), ["hold", "cancel"]);
    assert.deepEqual(allowedActions(task({ held: true })), ["unhold", "cancel"]);
  });
  it("running: solo annulla", () => {
    assert.deepEqual(allowedActions(task({ status: "running" })), ["cancel"]);
  });
  it("review: riprova, fatto, annulla", () => {
    assert.deepEqual(allowedActions(task({ status: "review" })), ["retry", "done", "cancel"]);
  });
  it("failed e blocked: riprova, fatto, annulla", () => {
    for (const status of ["failed", "blocked"]) {
      assert.deepEqual(allowedActions(task({ status })), ["retry", "done", "cancel"]);
    }
  });
  it("cancelled: solo riprova (gia' annullato)", () => {
    assert.deepEqual(allowedActions(task({ status: "cancelled" })), ["retry"]);
  });
  it("done: nessuna azione", () => {
    assert.deepEqual(allowedActions(task({ status: "done" })), []);
  });
  it("hold/unhold mai fuori da queued", () => {
    for (const status of ["running", "review", "done", "failed", "blocked", "cancelled"]) {
      const a = allowedActions(task({ status, held: true }));
      assert.ok(!a.includes("hold") && !a.includes("unhold"), status);
    }
  });
  it("ogni azione ha un'etichetta", () => {
    for (const a of ["hold", "unhold", "retry", "cancel", "done"]) assert.ok(ACTION_LABELS[a]);
  });
  it("apri terminale solo con una sessione", () => {
    assert.equal(canOpenTerminal({ session: "t1" }), true);
    assert.equal(canOpenTerminal({ session: null }), false);
    assert.equal(canOpenTerminal({}), false);
  });
});

describe("board: conteggi", () => {
  const tasks = [
    task({ status: "done" }),
    task({ status: "running" }),
    task({ status: "review" }),
    task({ status: "queued" }),
    task({ status: "cancelled" }),
  ];
  it("avanzamento: solo i done", () => {
    assert.deepEqual(progress(tasks), { done: 1, total: 5 });
    assert.deepEqual(progress([]), { done: 0, total: 0 });
  });
  it("task attivi: running + review", () => {
    assert.equal(activeCount({ tasks }), 2);
    assert.equal(activeCount(null), 0);
    assert.equal(activeCount(undefined), 0);
  });
  it("max_parallel limitato a 1..16", () => {
    assert.equal(clampMaxParallel(0), 1);
    assert.equal(clampMaxParallel(99), 16);
    assert.equal(clampMaxParallel(5), 5);
    assert.equal(clampMaxParallel(NaN), 1);
  });
});

describe("board: sotto-stato e report", () => {
  it("sotto-stato solo per i task in corso", () => {
    assert.equal(
      workerSubStatus(task({ status: "running", worker_status: "needs_input" })).text,
      "aspetta",
    );
    assert.equal(workerSubStatus(task({ status: "running", worker_status: null })), null);
    assert.equal(workerSubStatus(task({ status: "done", worker_status: "completed" })), null);
  });
  it("report troncato", () => {
    assert.equal(shortReport({ last_report: null }), "");
    assert.equal(shortReport({ last_report: "x".repeat(300) }, 10).length, 10);
  });
  it("board di un altro workspace", () => {
    assert.equal(isBoardOf(emptyBoard("w1"), "w1"), true);
    assert.equal(isBoardOf(emptyBoard("w1"), "w2"), false);
  });
});

describe("boardLayout", () => {
  const tasks = [
    { id: "db", depends_on: [] },
    { id: "api", depends_on: ["db"] },
    { id: "ui", depends_on: ["api", "db", "ghost"] },
  ];
  it("archi dalla dipendenza al dipendente, ignorando id inesistenti", () => {
    assert.deepEqual(
      boardEdges(tasks).map((e) => [e.source, e.target]),
      [
        ["db", "api"],
        ["api", "ui"],
        ["db", "ui"],
      ],
    );
  });
  it("layout sinistra-destra: i dipendenti stanno a destra", () => {
    const { nodes } = layoutBoard(tasks);
    const x = Object.fromEntries(nodes.map((n) => [n.id, n.position.x]));
    assert.ok(x.db + NODE_WIDTH <= x.api);
    assert.ok(x.api + NODE_WIDTH <= x.ui);
  });
  it("layout deterministico", () => {
    assert.deepEqual(layoutBoard(tasks), layoutBoard(tasks));
  });
  it("la firma ignora lo stato, non la struttura", () => {
    const a = layoutSignature(tasks);
    assert.equal(a, layoutSignature(tasks.map((t) => ({ ...t, status: "done" }))));
    assert.notEqual(a, layoutSignature([...tasks, { id: "x", depends_on: [] }]));
  });
  it("board vuota", () => {
    assert.deepEqual(layoutBoard([]), { nodes: [], edges: [] });
  });
});

describe("board: merge", () => {
  it("una board più vecchia non sostituisce quella nota", () => {
    const a = { ...emptyBoard("w"), updated_at: 5 };
    const b = { ...emptyBoard("w"), updated_at: 4 };
    assert.equal(mergeBoard(a, b), a);
    assert.equal(mergeBoard(b, a), a);
    assert.equal(mergeBoard(undefined, b), b);
    assert.equal(mergeBoard(a, { ...a }).updated_at, 5);
  });

  it("con rev: la piu' bassa viene scartata, l'uguale tiene la nota", () => {
    const cur = { ...emptyBoard("w"), updated_at: 5, rev: 7 };
    const older = { ...emptyBoard("w"), updated_at: 9, rev: 6 };
    const same = { ...cur };
    const newer = { ...emptyBoard("w"), updated_at: 1, rev: 8 };
    assert.equal(mergeBoard(cur, older), cur);
    assert.equal(mergeBoard(cur, same), cur);
    assert.equal(mergeBoard(cur, newer), newer);
  });

  it("senza rev: ripiego su updated_at", () => {
    const cur = { ...emptyBoard("w"), updated_at: 5, rev: 7 };
    const noRev = { ...emptyBoard("w"), updated_at: 6 };
    assert.equal(mergeBoard(cur, noRev), noRev);
  });
});

describe("board: setup dei worktree", () => {
  it("trim, vuoto = nessuno, massimo 1000", () => {
    assert.equal(sanitizeWorktreeSetup("  npm ci  "), "npm ci");
    assert.equal(sanitizeWorktreeSetup("   "), "");
    assert.equal(sanitizeWorktreeSetup("x".repeat(2000)).length, WORKTREE_SETUP_MAX);
  });
});

describe("board: chiave degli id", () => {
  it("indipendente dall'ordine e dallo stato, cambia con gli id", () => {
    const a = [{ id: "a", status: "queued" }, { id: "b", status: "queued" }];
    const b = [{ id: "b", status: "done" }, { id: "a", status: "running" }];
    assert.equal(taskIdsKey(a), taskIdsKey(b));
    assert.notEqual(taskIdsKey(a), taskIdsKey([...a, { id: "c" }]));
    assert.equal(taskIdsKey([]), "");
  });
});

describe("canEditModel", () => {
  it("consente il cambio solo per queued, failed, blocked e cancelled", () => {
    for (const s of ["queued", "failed", "blocked", "cancelled"]) assert.equal(canEditModel(s), true, s);
    for (const s of ["running", "review", "done"]) assert.equal(canEditModel(s), false, s);
  });
});
