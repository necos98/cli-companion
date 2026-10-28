import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  DEFAULT_SORT,
  MANAGER_PREFS_KEY,
  acceptsDrop,
  defaultPrefs,
  isDraggable,
  isTypingTarget,
  kanbanColumns,
  kanbanMeta,
  listRows,
  listSubtitle,
  queuePositions,
  readPrefs,
  reorderTarget,
  sortTasks,
  statusCounts,
  toggleSort,
  writePrefs,
} from "../src/lib/taskViews.ts";
import {
  buildPatch,
  dependencyOptions,
  findCreatedTask,
  formFromTask,
  hasErrors,
  isMultiRepo,
  repoOptions,
  repoValue,
  validateTaskForm,
} from "../src/lib/taskForm.ts";

const DAY = 86_400_000;
const NOW = 100 * DAY;

const task = (id, over = {}) => ({
  id,
  title: `Task ${id}`,
  task: `spec ${id}`,
  depends_on: [],
  agent: "claude-code",
  status: "queued",
  attempts: 0,
  created_at: 1,
  updated_at: 1,
  waiting_on: [],
  attention: false,
  ...over,
});

const memory = (initial) => {
  const data = new Map(initial === undefined ? [] : [[MANAGER_PREFS_KEY, initial]]);
  return { getItem: (k) => data.get(k) ?? null, setItem: (k, v) => void data.set(k, v), data };
};

describe("kanban", () => {
  const tasks = [
    task("q1"),
    task("q2", { title: "Importa dati" }),
    task("r1", { status: "running", started_at: NOW - 60_000 }),
    task("d-new", { status: "done", finished_at: NOW - DAY }),
    task("d-newer", { status: "done", finished_at: NOW - 1000 }),
    task("d-old", { status: "done", finished_at: NOW - 10 * DAY }),
    task("f1", { status: "failed", updated_at: 5 }),
    task("f2", { status: "failed", updated_at: 9 }),
  ];
  it("colonne: coda in ordine, fatti degli ultimi 7 giorni dal più recente, falliti dal più recente", () => {
    const c = kanbanColumns(tasks, "", NOW);
    assert.deepEqual(c.queued.map((t) => t.id), ["q1", "q2"]);
    assert.deepEqual(c.running.map((t) => t.id), ["r1"]);
    assert.deepEqual(c.done.map((t) => t.id), ["d-newer", "d-new"]);
    assert.equal(c.doneOlder, 1);
    assert.deepEqual(c.failed.map((t) => t.id), ["f2", "f1"]);
  });
  it("la ricerca filtra le card", () => {
    const c = kanbanColumns(tasks, "importa", NOW);
    assert.deepEqual(c.queued.map((t) => t.id), ["q2"]);
    assert.equal(c.running.length, 0);
    assert.equal(c.doneOlder, 0);
  });
  it("drop: solo Da fare accetta, da Da fare e Fallito", () => {
    assert.equal(acceptsDrop("queued", "queued"), true);
    assert.equal(acceptsDrop("failed", "queued"), true);
    assert.equal(acceptsDrop("failed", "failed"), false);
    assert.equal(acceptsDrop("queued", "running"), false);
    assert.equal(acceptsDrop("running", "queued"), false);
    assert.equal(isDraggable("done"), false);
    assert.equal(isDraggable("running"), false);
  });
  it("riordino: prima / dopo un altro, in fondo, nessun cambiamento", () => {
    const q = ["a", "b", "c", "d"];
    assert.deepEqual(reorderTarget(q, "d", "b", false), { beforeId: "b" });
    assert.equal(reorderTarget(q, "a", "b", false), null);
    assert.deepEqual(reorderTarget(q, "a", "b", true), { beforeId: "c" });
    assert.equal(reorderTarget(q, "b", "a", true), null);
    assert.deepEqual(reorderTarget(q, "a", "c", true), { beforeId: "d" });
    assert.deepEqual(reorderTarget(q, "a", "d", true), { beforeId: null });
    assert.deepEqual(reorderTarget(q, "a", null, true), { beforeId: null });
    assert.equal(reorderTarget(q, "d", null, true), null);
    assert.equal(reorderTarget(q, "b", "b", false), null);
    assert.deepEqual(reorderTarget(q, "b", "a", false), { beforeId: "a" });
    assert.equal(reorderTarget(q, "x", "a", false), null);
  });
  it("meta: modello, tempo o esito", () => {
    assert.deepEqual(kanbanMeta(task("a", { model: "opus" }), NOW), ["opus"]);
    assert.deepEqual(kanbanMeta(task("a", { attempts: 1 }), NOW), ["tentativo 2"]);
    assert.deepEqual(
      kanbanMeta(task("a", { status: "running", model: "sonnet", started_at: NOW - 12 * 60_000 }), NOW),
      ["sonnet", "12m"],
    );
    assert.deepEqual(kanbanMeta(task("a", { status: "done", finished_at: NOW - 2 * 3_600_000 }), NOW), ["2 h fa"]);
    assert.deepEqual(
      kanbanMeta(task("a", { status: "failed", attempts: 1, updated_at: NOW - 5 * 60_000 }), NOW),
      ["tentativo 1", "aggiornato 5 min fa"],
    );
  });
});

describe("lista", () => {
  const tasks = [
    task("a", { title: "Zeta", updated_at: 10, status: "done", started_at: 1000, finished_at: 3000 }),
    task("b", { title: "alfa", updated_at: 30, status: "running", started_at: NOW - 10_000 }),
    task("c", { title: "Beta", updated_at: 20, status: "queued", repo: "api" }),
    task("d", { title: "Delta", updated_at: 5, status: "failed", started_at: 1000, finished_at: 31_000 }),
  ];
  it("ordinamento di default: aggiornati per primi", () => {
    assert.deepEqual(sortTasks(tasks, DEFAULT_SORT, NOW).map((t) => t.id), ["b", "c", "a", "d"]);
    assert.deepEqual(tasks.map((t) => t.id), ["a", "b", "c", "d"]);
  });
  it("per titolo ignora le maiuscole; per durata i senza durata vanno sempre in fondo", () => {
    assert.deepEqual(sortTasks(tasks, { key: "title", desc: false }, NOW).map((t) => t.id), ["b", "c", "d", "a"]);
    assert.deepEqual(sortTasks(tasks, { key: "duration", desc: true }, NOW).map((t) => t.id), ["d", "b", "a", "c"]);
    assert.deepEqual(sortTasks(tasks, { key: "duration", desc: false }, NOW).map((t) => t.id), ["a", "b", "d", "c"]);
  });
  it("toggleSort", () => {
    assert.deepEqual(toggleSort(DEFAULT_SORT, "updated"), { key: "updated", desc: false });
    assert.deepEqual(toggleSort(DEFAULT_SORT, "title"), { key: "title", desc: false });
    assert.deepEqual(toggleSort({ key: "title", desc: false }, "duration"), { key: "duration", desc: true });
  });
  it("conteggi: seguono ricerca e repo, non gli stati", () => {
    assert.deepEqual(statusCounts(tasks, { query: "", repos: [] }), { queued: 1, running: 1, done: 1, failed: 1 });
    assert.deepEqual(statusCounts(tasks, { query: "alfa", repos: [] }), { queued: 0, running: 1, done: 0, failed: 0 });
    assert.deepEqual(statusCounts(tasks, { query: "", repos: ["api"] }), { queued: 1, running: 0, done: 0, failed: 0 });
  });
  it("righe: filtri e ordinamento", () => {
    const rows = listRows(tasks, { query: "", statuses: ["done", "failed"], repos: [] }, DEFAULT_SORT, NOW);
    assert.deepEqual(rows.map((t) => t.id), ["a", "d"]);
  });
  it("sottotitolo per stato", () => {
    const all = [task("x"), task("y", { waiting_on: ["x"] }), task("z")];
    const pos = queuePositions(all);
    assert.equal(listSubtitle(all[0], all, pos), "Prossimo in coda");
    assert.equal(listSubtitle(all[1], all, pos), "attende Task x");
    assert.equal(listSubtitle(all[2], all, pos), "In coda, posizione 3");
    assert.equal(
      listSubtitle(task("d", { status: "done", report: { status: "completed", summary: "ok" } }), all, pos),
      "ok",
    );
    assert.equal(listSubtitle(task("f", { status: "failed", note: "stopped by the user" }), all, pos), "stopped by the user");
  });
});

describe("preferenze", () => {
  it("default con storage assente, vuoto o rotto", () => {
    assert.deepEqual(readPrefs(null, "w"), defaultPrefs());
    assert.deepEqual(readPrefs(memory(), "w"), defaultPrefs());
    assert.deepEqual(readPrefs(memory("{rotto"), "w"), defaultPrefs());
    assert.deepEqual(readPrefs(memory("[]"), "w"), defaultPrefs());
  });
  it("scrittura e lettura per workspace", () => {
    const m = memory();
    const p = { view: "list", list: { statuses: ["done"], repos: ["api"], sort: { key: "title", desc: false } } };
    writePrefs(m, "w1", p);
    writePrefs(m, "w2", { ...defaultPrefs(), view: "graph" });
    assert.deepEqual(readPrefs(m, "w1"), p);
    assert.equal(readPrefs(m, "w2").view, "graph");
    assert.deepEqual(readPrefs(m, "w3"), defaultPrefs());
  });
  it("valori non validi scartati", () => {
    const m = memory(
      JSON.stringify({
        w: { view: "boh", list: { statuses: ["done", "x", 3], repos: ["a", 1], sort: { key: "zzz" } } },
      }),
    );
    const p = readPrefs(m, "w");
    assert.equal(p.view, "kanban");
    assert.deepEqual(p.list.statuses, ["done"]);
    assert.deepEqual(p.list.repos, ["a"]);
    assert.deepEqual(p.list.sort, DEFAULT_SORT);
  });
  it("accessor che lancia: nessun errore", () => {
    const boom = {
      getItem() {
        throw new Error("blocked");
      },
      setItem() {
        throw new Error("blocked");
      },
    };
    assert.deepEqual(readPrefs(boom, "w"), defaultPrefs());
    assert.doesNotThrow(() => writePrefs(boom, "w", defaultPrefs()));
  });
});

describe("tastiera", () => {
  it("campi di testo", () => {
    assert.equal(isTypingTarget({ tagName: "INPUT" }), true);
    assert.equal(isTypingTarget({ tagName: "textarea" }), true);
    assert.equal(isTypingTarget({ tagName: "DIV", isContentEditable: true }), true);
    assert.equal(isTypingTarget({ tagName: "BUTTON" }), false);
    assert.equal(isTypingTarget(null), false);
  });
});

describe("form task", () => {
  it("validazione", () => {
    assert.equal(hasErrors(validateTaskForm({ title: "x", task: "y", model: "", depends_on: [], kind: "work" })), false);
    const e = validateTaskForm({ title: "  ", task: "", model: "no good!", depends_on: [], kind: "work" });
    assert.ok(e.title && e.task && e.model);
    assert.ok(validateTaskForm({ title: "x".repeat(81), task: "y", model: "", depends_on: [], kind: "work" }).title);
  });
  it("dipendenze: niente Fatti, niente se stesso, niente cicli", () => {
    const ts = [
      task("a"),
      task("b", { depends_on: ["a"] }),
      task("c", { depends_on: ["b"] }),
      task("d"),
      task("e", { status: "done" }),
    ];
    assert.deepEqual(dependencyOptions(ts, null).map((t) => t.id), ["a", "b", "c", "d"]);
    // Per "a": b e c dipendono da lui (anche indirettamente).
    assert.deepEqual(dependencyOptions(ts, "a").map((t) => t.id), ["d"]);
    assert.deepEqual(dependencyOptions(ts, "c").map((t) => t.id), ["a", "b", "d"]);
    // Un Fatto già selezionato resta disponibile per essere tolto.
    assert.deepEqual(dependencyOptions(ts, "d", ["e"]).map((t) => t.id), ["a", "b", "c", "e"]);
  });
  it("repository", () => {
    assert.deepEqual(repoOptions([{ repo: "web" }, { repo: null }], ["api", "."]), [".", "api", "web"]);
    assert.equal(isMultiRepo(["."]), false);
    assert.equal(isMultiRepo([".", "api"]), true);
    assert.equal(repoValue("."), null);
    assert.equal(repoValue("api"), "api");
  });
  it("patch: solo i campi cambiati", () => {
    const t = task("a", { depends_on: ["x", "y"], model: "opus", repo: "api" });
    const same = formFromTask(t);
    assert.deepEqual(buildPatch(t, same, true), {});
    assert.deepEqual(buildPatch(t, { ...same, title: " Nuovo ", depends_on: ["y", "x"] }, true), { title: "Nuovo" });
    assert.deepEqual(buildPatch(t, { ...same, depends_on: ["x"], model: "", repo: "." }, true), {
      depends_on: ["x"],
      model: null,
      repo: null,
    });
    // Senza repo modificabile il repo non entra nella patch.
    assert.deepEqual(buildPatch(t, { ...same, repo: "." }, false), {});
  });
  it("task appena creato", () => {
    const board = {
      tasks: [
        task("old", { title: "Stesso", created_at: 5 }),
        task("stesso-2", { title: "Stesso", created_at: 9 }),
        task("other", { title: "Altro", created_at: 20 }),
      ],
    };
    assert.equal(findCreatedTask(board, "Stesso", new Set(["old", "other"]))?.id, "stesso-2");
    assert.equal(findCreatedTask(board, "Stesso", new Set(["old", "stesso-2", "other"]))?.id, "stesso-2");
    assert.equal(findCreatedTask(board, "Inesistente", new Set()), null);
  });
});
