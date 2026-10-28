import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  ACTION_LABELS,
  DEFAULT_DELIVERY,
  DONE_RECENT_DAYS,
  EMPTY_FILTER,
  STATUSES,
  WORKER_RULES_MAX,
  activeCount,
  allowedActions,
  attentionCount,
  canEditTask,
  cascadeDelete,
  canOpenTerminal,
  clampMaxParallel,
  deliveryHelp,
  deliveryLabel,
  dependencyCandidates,
  displayStatus,
  doneTasks,
  effectiveDelivery,
  emptyBoard,
  failedTasks,
  filterTasks,
  formatDuration,
  formatRelative,
  formatTaskDuration,
  isBoardOf,
  isFiltering,
  isParallel,
  matchesQuery,
  mergeBoard,
  mergeMethodLabel,
  normalizeDelivery,
  prUrl,
  progress,
  queuedTasks,
  runningTasks,
  sanitizeWorkerRules,
  shortReport,
  sortByUpdated,
  statusBorderClass,
  statusDotClass,
  statusHex,
  statusLabel,
  statusTextClass,
  taskCounts,
  taskIdsKey,
  taskRepos,
  toggleStatus,
  updatedLabel,
  waitingOnLabel,
  workerSubStatus,
} from "../src/lib/board.ts";
import { boardEdges, layoutBoard, layoutSignature, NODE_WIDTH } from "../src/lib/boardLayout.ts";

const NOW = 1_800_000_000_000;
const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 24 * HOUR;

const task = (extra = {}) => ({
  id: "a",
  title: "Task A",
  task: "Specifica di A",
  status: "queued",
  depends_on: [],
  waiting_on: [],
  attention: false,
  attempts: 0,
  created_at: NOW - DAY,
  updated_at: NOW - HOUR,
  ...extra,
});

describe("board: stati", () => {
  it("etichette italiane dei quattro stati e dell'attenzione", () => {
    assert.deepEqual(STATUSES.map(statusLabel), ["Da fare", "In corso", "Fatto", "Fallito"]);
    assert.equal(statusLabel("attention"), "Richiede attenzione");
  });
  it("colori della palette: grigio, ambra, verde, rosso, arancione", () => {
    assert.equal(statusTextClass("queued"), "text-muted-foreground");
    assert.equal(statusTextClass("running"), "text-status-running");
    assert.equal(statusTextClass("done"), "text-status-finished");
    assert.equal(statusTextClass("failed"), "text-destructive");
    assert.equal(statusTextClass("attention"), "text-status-attention");
    assert.equal(statusDotClass("running"), "bg-status-running");
    assert.match(statusBorderClass("failed"), /destructive/);
    assert.equal(statusHex("done"), "#34d399");
  });
  it("il blu (primary) non e' mai un colore di stato", () => {
    for (const s of [...STATUSES, "attention"]) {
      assert.doesNotMatch(statusTextClass(s) + statusDotClass(s) + statusBorderClass(s), /primary/);
    }
  });
  it("stato sconosciuto: etichetta invariata e colore neutro", () => {
    assert.equal(statusLabel("boh"), "boh");
    assert.equal(statusTextClass("boh"), "text-muted-foreground");
  });
  it("attention solo per un task in corso con il flag", () => {
    assert.equal(displayStatus(task({ status: "running", attention: true })), "attention");
    assert.equal(displayStatus(task({ status: "running" })), "running");
    assert.equal(displayStatus(task({ status: "queued", attention: true })), "queued");
    assert.equal(displayStatus(task({ status: "failed" })), "failed");
  });
});

describe("board: selettori", () => {
  const board = {
    proposals: [{ id: "p1" }],
    tasks: [
      task({ id: "q1" }),
      task({ id: "r1", status: "running" }),
      task({ id: "d-old", status: "done", finished_at: NOW - 10 * DAY }),
      task({ id: "q2" }),
      task({ id: "d-new", status: "done", finished_at: NOW - HOUR }),
      task({ id: "f1", status: "failed" }),
      task({ id: "r2", status: "running", attention: true }),
      task({ id: "d-mid", status: "done", finished_at: NOW - 2 * DAY }),
    ],
  };
  it("da fare e in corso nell'ordine dell'array", () => {
    assert.deepEqual(queuedTasks(board).map((t) => t.id), ["q1", "q2"]);
    assert.deepEqual(runningTasks(board).map((t) => t.id), ["r1", "r2"]);
    assert.deepEqual(failedTasks(board).map((t) => t.id), ["f1"]);
  });
  it("fatti: ultimi 7 giorni, dal piu' recente; senza limite tutti", () => {
    assert.equal(DONE_RECENT_DAYS, 7);
    assert.deepEqual(doneTasks(board, NOW).map((t) => t.id), ["d-new", "d-mid"]);
    assert.deepEqual(doneTasks(board, NOW, null).map((t) => t.id), ["d-new", "d-mid", "d-old"]);
    assert.deepEqual(doneTasks(board, NOW, 1).map((t) => t.id), ["d-new"]);
  });
  it("un fatto senza finished_at usa updated_at", () => {
    const b = { tasks: [task({ status: "done", finished_at: null, updated_at: NOW - DAY })] };
    assert.equal(doneTasks(b, NOW).length, 1);
    assert.equal(doneTasks(b, NOW, 0.5).length, 0);
  });
  it("conteggi, attenzione e attivi", () => {
    assert.deepEqual(taskCounts(board), {
      proposals: 1,
      queued: 2,
      running: 2,
      done: 3,
      failed: 1,
      attention: 1,
      total: 8,
    });
    assert.equal(attentionCount(board), 1);
    assert.equal(activeCount(board), 2);
    assert.equal(activeCount(null), 0);
    assert.equal(activeCount(undefined), 0);
    assert.equal(taskCounts(null).total, 0);
  });
  it("avanzamento: solo i fatti", () => {
    assert.deepEqual(progress(board.tasks), { done: 3, total: 8 });
    assert.deepEqual(progress([]), { done: 0, total: 0 });
  });
  it("dipendenze possibili: tutto tranne i fatti", () => {
    assert.deepEqual(dependencyCandidates(board).map((t) => t.id), ["q1", "r1", "q2", "f1", "r2"]);
  });
  it("repository dei task (root come '.')", () => {
    assert.deepEqual(taskRepos([{ repo: null }, { repo: "api" }, {}, { repo: "api" }]), [".", "api"]);
  });
  it("max_parallel limitato a 1..16", () => {
    assert.equal(clampMaxParallel(0), 1);
    assert.equal(clampMaxParallel(99), 16);
    assert.equal(clampMaxParallel(5), 5);
    assert.equal(clampMaxParallel(NaN), 1);
  });
  it("chiave degli id: indipendente da ordine e stato", () => {
    const a = [{ id: "a", status: "queued" }, { id: "b", status: "queued" }];
    const b = [{ id: "b", status: "done" }, { id: "a", status: "running" }];
    assert.equal(taskIdsKey(a), taskIdsKey(b));
    assert.notEqual(taskIdsKey(a), taskIdsKey([...a, { id: "c" }]));
    assert.equal(taskIdsKey([]), "");
  });
});

describe("board: azioni", () => {
  it("azioni per stato", () => {
    assert.deepEqual(allowedActions(task()), ["to_top", "delete"]);
    assert.deepEqual(allowedActions(task({ status: "running" })), ["stop"]);
    assert.deepEqual(allowedActions(task({ status: "failed" })), ["retry", "mark_done", "delete"]);
    assert.deepEqual(allowedActions(task({ status: "done" })), []);
  });
  it("ogni azione ha un'etichetta italiana", () => {
    assert.deepEqual(ACTION_LABELS, {
      retry: "Riprova",
      mark_done: "Segna come fatto",
      stop: "Ferma",
      delete: "Elimina",
      delete_cascade: "Elimina tutti",
      to_top: "In cima alla coda",
    });
  });
  it("si modifica solo un Da fare o un Fallito", () => {
    for (const s of ["queued", "failed"]) assert.equal(canEditTask({ status: s }), true, s);
    for (const s of ["running", "done"]) assert.equal(canEditTask({ status: s }), false, s);
  });
  it("eliminazione a cascata: task e dipendenti non fatti, diretti e indiretti", () => {
    const tasks = [
      task({ id: "a" }),
      task({ id: "b", depends_on: ["a"] }),
      task({ id: "c", depends_on: ["a"], status: "failed" }),
      task({ id: "d", depends_on: ["b", "c"] }),
      task({ id: "e", depends_on: ["d"] }),
      task({ id: "x" }),
      task({ id: "y", depends_on: ["x"] }),
    ];
    assert.deepEqual(cascadeDelete("a", { tasks }), { dependents: ["b", "c", "d", "e"], running: [] });
    assert.deepEqual(cascadeDelete("b", { tasks }), { dependents: ["d", "e"], running: [] });
    assert.deepEqual(cascadeDelete("e", { tasks }), { dependents: [], running: [] });
    assert.deepEqual(cascadeDelete("x", { tasks }), { dependents: ["y"], running: [] });
  });
  it("eliminazione a cascata: un dipendente Fatto resta e non si attraversa", () => {
    const tasks = [
      task({ id: "a" }),
      task({ id: "b", depends_on: ["a"], status: "done" }),
      task({ id: "c", depends_on: ["b"] }),
    ];
    assert.deepEqual(cascadeDelete("a", { tasks }), { dependents: [], running: [] });
  });
  it("eliminazione a cascata: i task in corso dell'insieme bloccano", () => {
    const tasks = [
      task({ id: "a" }),
      task({ id: "b", depends_on: ["a"] }),
      task({ id: "c", depends_on: ["b"], status: "running" }),
      task({ id: "d", depends_on: ["b"], status: "running" }),
      task({ id: "z", status: "running" }),
    ];
    assert.deepEqual(cascadeDelete("a", { tasks }), { dependents: ["b", "c", "d"], running: ["c", "d"] });
    assert.deepEqual(cascadeDelete("a", { tasks: [task({ id: "a" })] }).running, []);
    // depends_on assente (vista parziale): nessun dipendente
    assert.deepEqual(cascadeDelete("a", { tasks: [{ id: "a", status: "queued" }, { id: "b", status: "queued" }] }), {
      dependents: [],
      running: [],
    });
  });
  it("apri terminale solo con una sessione", () => {
    assert.equal(canOpenTerminal({ session: "t1" }), true);
    assert.equal(canOpenTerminal({ session: null }), false);
    assert.equal(canOpenTerminal({ session: "" }), false);
    assert.equal(canOpenTerminal({}), false);
  });
  it("link della PR solo se http(s)", () => {
    assert.equal(prUrl({ report: { pr_url: "https://github.com/o/r/pull/3" } }), "https://github.com/o/r/pull/3");
    assert.equal(prUrl({ report: { pr_url: "javascript:alert(1)" } }), null);
    assert.equal(prUrl({ report: null }), null);
    assert.equal(prUrl({}), null);
  });
});

describe("board: testi", () => {
  const all = [
    { id: "a", title: "Schema" },
    { id: "b", title: "Vista Kanban" },
  ];
  it("attende X / attende N task", () => {
    assert.equal(waitingOnLabel({ status: "queued", waiting_on: ["b"] }, all), "attende Vista Kanban");
    assert.equal(waitingOnLabel({ status: "queued", waiting_on: ["a", "b"] }, all), "attende 2 task");
    assert.equal(waitingOnLabel({ status: "queued", waiting_on: ["x"] }, all), "attende x");
    assert.equal(waitingOnLabel({ status: "queued", waiting_on: [] }, all), null);
    assert.equal(waitingOnLabel({ status: "running", waiting_on: ["a"] }, all), null);
  });
  it("durata compatta", () => {
    assert.equal(formatDuration(0), "0s");
    assert.equal(formatDuration(45_000), "45s");
    assert.equal(formatDuration(12 * MIN + 5_000), "12m");
    assert.equal(formatDuration(HOUR + 5 * MIN), "1h 05m");
    assert.equal(formatDuration(2 * DAY + 3 * HOUR), "2g 3h");
    assert.equal(formatDuration(-5), "0s");
  });
  it("durata di un task: in corso fino a ora, concluso fino alla fine", () => {
    const running = task({ status: "running", started_at: NOW - 12 * MIN });
    assert.equal(formatTaskDuration(running, NOW), "12m");
    const done = task({ status: "done", started_at: NOW - HOUR, finished_at: NOW - HOUR + 9 * MIN });
    assert.equal(formatTaskDuration(done, NOW), "9m");
    assert.equal(formatTaskDuration(task({ status: "queued" }), NOW), "");
    assert.equal(formatTaskDuration(task({ status: "failed" }), NOW), "");
  });
  it("aggiornato: tempo relativo in italiano", () => {
    assert.equal(formatRelative(NOW - 10_000, NOW), "adesso");
    assert.equal(formatRelative(NOW - 20 * MIN, NOW), "20 min fa");
    assert.equal(formatRelative(NOW - 2 * HOUR, NOW), "2 h fa");
    assert.equal(formatRelative(NOW - DAY, NOW), "ieri");
    assert.equal(formatRelative(NOW - 3 * DAY, NOW), "3 g fa");
    assert.match(formatRelative(NOW - 30 * DAY, NOW), /^\d\d\/\d\d\/\d{4}$/);
    assert.equal(updatedLabel({ updated_at: NOW - 20 * MIN }, NOW), "aggiornato 20 min fa");
  });
  it("sotto-stato solo per i task in corso", () => {
    assert.equal(workerSubStatus(task({ status: "running", worker_status: "needs_input" })).text, "aspetta");
    assert.equal(workerSubStatus(task({ status: "running", worker_status: null })), null);
    assert.equal(workerSubStatus(task({ status: "done", worker_status: "completed" })), null);
    const prep = workerSubStatus(task({ status: "running", worker_status: "starting", preparing: "setup 1/2" }));
    assert.equal(prep.text, "Preparazione…");
    assert.equal(workerSubStatus(task({ status: "failed", preparing: "x" })), null);
  });
  it("report troncato", () => {
    assert.equal(shortReport({ last_report: null }), "");
    assert.equal(shortReport({ last_report: "x".repeat(300) }, 10).length, 10);
  });
});

describe("board: ricerca e filtri", () => {
  const t = task({
    id: "kanban-view",
    title: "Vista Kanban",
    task: "Implementa le colonne con drag and drop",
    note: "interrupted: the app was restarted",
    report: { status: "completed", summary: "Aggiunta la Vista con 4 colonne" },
  });
  it("full-text su titolo, specifica, esito, nota e id, senza maiuscole", () => {
    for (const q of ["kanban", "DRAG", "4 colonne", "restarted", "kanban-view", "  vista  "]) {
      assert.equal(matchesQuery(t, q), true, q);
    }
    assert.equal(matchesQuery(t, "inesistente"), false);
    assert.equal(matchesQuery(t, ""), true);
    assert.equal(matchesQuery(t, "   "), true);
  });
  it("task senza report ne' nota non rompe la ricerca", () => {
    assert.equal(matchesQuery(task(), "specifica"), true);
    assert.equal(matchesQuery(task(), "esito"), false);
  });
  const tasks = [
    task({ id: "a", status: "queued", title: "Alfa" }),
    task({ id: "b", status: "running", title: "Beta", repo: "api" }),
    task({ id: "c", status: "failed", title: "Gamma", repo: "web" }),
    task({ id: "d", status: "done", title: "Alfa due", repo: "api" }),
  ];
  it("filtro per stato (selezione multipla), repo e testo", () => {
    assert.deepEqual(filterTasks(tasks, EMPTY_FILTER).map((x) => x.id), ["a", "b", "c", "d"]);
    assert.deepEqual(
      filterTasks(tasks, { ...EMPTY_FILTER, statuses: ["queued", "failed"] }).map((x) => x.id),
      ["a", "c"],
    );
    assert.deepEqual(filterTasks(tasks, { ...EMPTY_FILTER, repos: ["api"] }).map((x) => x.id), ["b", "d"]);
    assert.deepEqual(filterTasks(tasks, { ...EMPTY_FILTER, repos: ["."] }).map((x) => x.id), ["a"]);
    assert.deepEqual(
      filterTasks(tasks, { query: "alfa", statuses: ["done"], repos: [] }).map((x) => x.id),
      ["d"],
    );
  });
  it("toggle dei chip e filtri attivi", () => {
    assert.deepEqual(toggleStatus([], "done"), ["done"]);
    assert.deepEqual(toggleStatus(["done", "failed"], "done"), ["failed"]);
    assert.equal(isFiltering(EMPTY_FILTER), false);
    assert.equal(isFiltering({ ...EMPTY_FILTER, query: " x " }), true);
    assert.equal(isFiltering({ ...EMPTY_FILTER, statuses: ["done"] }), true);
    assert.equal(isFiltering({ ...EMPTY_FILTER, repos: ["api"] }), true);
  });
  it("ordine per aggiornamento: discendente di default, senza mutare l'input", () => {
    const x = [task({ id: "o", updated_at: 1 }), task({ id: "n", updated_at: 9 })];
    assert.deepEqual(sortByUpdated(x).map((t2) => t2.id), ["n", "o"]);
    assert.deepEqual(sortByUpdated(x, false).map((t2) => t2.id), ["o", "n"]);
    assert.equal(x[0].id, "o");
  });
});

describe("board: consegna", () => {
  it("etichette italiane delle quattro modalita'", () => {
    assert.equal(deliveryLabel("pr"), "Branch + PR + merge");
    assert.equal(deliveryLabel("local_merge"), "Branch + merge locale");
    assert.equal(deliveryLabel("push"), "Commit + push");
    assert.equal(deliveryLabel("commit"), "Solo commit");
    assert.equal(deliveryLabel("boh"), "boh");
    for (const m of ["pr", "local_merge", "push", "commit"]) assert.ok(deliveryHelp(m).length > 10, m);
    assert.equal(mergeMethodLabel("squash"), "Squash");
  });
  it("parallelo solo con pr e local_merge", () => {
    assert.equal(isParallel({ mode: "pr", parallel: true }), true);
    assert.equal(isParallel({ mode: "local_merge", parallel: true }), true);
    assert.equal(isParallel({ mode: "pr", parallel: false }), false);
    assert.equal(isParallel({ mode: "push", parallel: true }), false);
    assert.equal(isParallel({ mode: "commit", parallel: true }), false);
  });
  it("normalizzazione: il parallelo cade fuori dai branch", () => {
    const d = { mode: "push", parallel: true, merge_method: "squash" };
    assert.equal(normalizeDelivery(d).parallel, false);
    assert.equal(normalizeDelivery({ ...d, mode: "pr" }).parallel, true);
  });
  it("consegna effettiva: assente = solo commit seriale", () => {
    assert.deepEqual(effectiveDelivery({}), DEFAULT_DELIVERY);
    assert.deepEqual(effectiveDelivery({ delivery: null }), DEFAULT_DELIVERY);
    assert.equal(effectiveDelivery({ delivery: { mode: "pr", parallel: true, merge_method: "rebase" } }).merge_method, "rebase");
    assert.equal(DEFAULT_DELIVERY.mode, "commit");
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
  it("board di un altro workspace", () => {
    assert.equal(isBoardOf(emptyBoard("w1"), "w1"), true);
    assert.equal(isBoardOf(emptyBoard("w1"), "w2"), false);
  });
  it("con rev: la piu' bassa viene scartata, l'uguale tiene la nota", () => {
    const cur = { ...emptyBoard("w"), updated_at: 5, rev: 7 };
    const older = { ...emptyBoard("w"), updated_at: 9, rev: 6 };
    const same = { ...cur };
    const newer = { ...emptyBoard("w"), updated_at: 1, rev: 8 };
    assert.equal(mergeBoard(cur, older), cur);
    assert.equal(mergeBoard(cur, same), cur);
    assert.equal(mergeBoard(cur, newer), newer);
    assert.equal(mergeBoard(undefined, older), older);
  });
  it("senza rev: ripiego su updated_at", () => {
    const a = { ...emptyBoard("w"), updated_at: 5, rev: undefined };
    const b = { ...emptyBoard("w"), updated_at: 4, rev: undefined };
    assert.equal(mergeBoard(a, b), a);
    assert.equal(mergeBoard(b, a), a);
    const cur = { ...emptyBoard("w"), updated_at: 5, rev: 7 };
    const noRev = { ...emptyBoard("w"), updated_at: 6, rev: undefined };
    assert.equal(mergeBoard(cur, noRev), noRev);
  });
  it("board vuota segnaposto con consegna di default", () => {
    const b = emptyBoard("w");
    assert.deepEqual(b.tasks, []);
    assert.deepEqual(b.proposals, []);
    assert.equal(b.delivery.mode, "commit");
    assert.equal(b.schedule_blocked, false);
  });
});

describe("board: regole dei worker", () => {
  it("sanitizeWorkerRules taglia e limita", () => {
    assert.equal(sanitizeWorkerRules("  no e2e \n"), "no e2e");
    assert.equal(sanitizeWorkerRules("a".repeat(WORKER_RULES_MAX + 10)).length, WORKER_RULES_MAX);
  });
});
