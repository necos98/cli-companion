import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  NO_GOAL,
  closedGoals,
  effectiveGoalFilter,
  filterByGoal,
  goalChoices,
  goalLabel,
  goalPhaseLabel,
  goalProgressLabel,
  goalRoundLabel,
  goalWaitingLabels,
  groupByGoal,
  groupByRound,
  openGoals,
} from "../src/lib/goals.ts";
import { needsYou, needsYouKey } from "../src/lib/needsYou.ts";
import { defaultPrefs, readPrefs, writePrefs } from "../src/lib/taskViews.ts";

const goal = (id, over = {}) => ({
  id,
  title: `Goal ${id}`,
  request: "richiesta",
  done_when: [],
  decisions: [],
  round: 1,
  created_at: 1,
  phase: "executing",
  waiting: [],
  tasks: { done: 0, total: 0 },
  ...over,
});

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
  blocked_by: [],
  attention: false,
  ...over,
});

const board = (over = {}) => ({ tasks: [], proposals: [], settings_proposals: [], goals: [], ...over });

describe("etichette dei goal", () => {
  it("fase, avanzamento e giro", () => {
    assert.equal(goalPhaseLabel(goal("a", { phase: "verifying" })), "In verifica");
    assert.equal(goalProgressLabel(goal("a", { tasks: { done: 3, total: 5 } })), "3/5");
    assert.equal(goalRoundLabel(goal("a", { round: 1 })), null);
    assert.equal(goalRoundLabel(goal("a", { round: 2 })), "giro 2");
  });

  it("ciò che aspetta l'utente", () => {
    assert.deepEqual(goalWaitingLabels(goal("a")), []);
    assert.equal(goalWaitingLabels(goal("a", { waiting: ["proposal", "failed"] })).length, 2);
  });

  it("titolo del goal di un task: l'id se il goal non c'è più, null se è sciolto", () => {
    const goals = [goal("a")];
    assert.equal(goalLabel(goals, "a"), "Goal a");
    assert.equal(goalLabel(goals, "vecchio"), "vecchio");
    assert.equal(goalLabel(goals, undefined), null);
  });
});

describe("ordinamento dei goal", () => {
  it("aperti: prima chi aspetta l'utente, poi dal più vecchio; i chiusi restano fuori", () => {
    const goals = [
      goal("tardi", { created_at: 30 }),
      goal("attesa", { created_at: 20, waiting: ["attention"] }),
      goal("presto", { created_at: 10 }),
      goal("chiuso", { created_at: 5, closed_at: 50, phase: "closed" }),
    ];
    assert.deepEqual(
      openGoals(goals).map((g) => g.id),
      ["attesa", "presto", "tardi"],
    );
  });

  it("chiusi: dal più recente; non modifica l'ingresso", () => {
    const goals = [goal("a", { closed_at: 10 }), goal("b", { closed_at: 30 }), goal("c")];
    assert.deepEqual(
      closedGoals(goals).map((g) => g.id),
      ["b", "a"],
    );
    assert.deepEqual(
      goals.map((g) => g.id),
      ["a", "b", "c"],
    );
  });
});

describe("filtro per goal", () => {
  const tasks = [task("1", { goal: "a" }), task("2", { goal: "b" }), task("3")];

  it("null = tutti, id = quel goal, NO_GOAL = i sciolti", () => {
    assert.equal(filterByGoal(tasks, null).length, 3);
    assert.deepEqual(
      filterByGoal(tasks, "a").map((t) => t.id),
      ["1"],
    );
    assert.deepEqual(
      filterByGoal(tasks, NO_GOAL).map((t) => t.id),
      ["3"],
    );
  });

  it("un goal sparito dalla board non nasconde i task", () => {
    const goals = [goal("a")];
    assert.equal(effectiveGoalFilter("a", goals), "a");
    assert.equal(effectiveGoalFilter("sparito", goals), null);
    assert.equal(effectiveGoalFilter(NO_GOAL, goals), NO_GOAL);
    assert.equal(effectiveGoalFilter(null, goals), null);
  });

  it("scelte: aperti, chiusi, poi i sciolti solo se servono", () => {
    const goals = [goal("a"), goal("z", { closed_at: 9 })];
    assert.deepEqual(goalChoices(goals, tasks), [
      { value: "a", label: "Goal a" },
      { value: "z", label: "Goal z (chiuso)" },
      { value: NO_GOAL, label: "Senza goal" },
    ]);
    assert.equal(goalChoices(goals, [task("1", { goal: "a" })]).length, 2);
    assert.deepEqual(goalChoices([], tasks), []);
  });
});

describe("raggruppamento", () => {
  it("per goal: aperti, chiusi, sconosciuti e infine i sciolti; niente gruppi vuoti", () => {
    const goals = [goal("chiuso", { closed_at: 9 }), goal("a", { created_at: 2 }), goal("vuoto")];
    const tasks = [
      task("1"),
      task("2", { goal: "chiuso" }),
      task("3", { goal: "a" }),
      task("4", { goal: "archiviato" }),
      task("5", { goal: "a" }),
    ];
    const groups = groupByGoal(tasks, goals);
    assert.deepEqual(
      groups.map((g) => [g.key, g.tasks.map((t) => t.id)]),
      [
        ["a", ["3", "5"]],
        ["chiuso", ["2"]],
        ["archiviato", ["4"]],
        [NO_GOAL, ["1"]],
      ],
    );
    assert.equal(groups[0].goal.id, "a");
    assert.equal(groups[2].goal, null);
    assert.equal(groups[2].title, "archiviato");
  });

  it("senza goal c'è il solo gruppo dei sciolti", () => {
    const groups = groupByGoal([task("1")], []);
    assert.equal(groups.length, 1);
    assert.equal(groups[0].goal, null);
  });

  it("per giro: crescente, il giro assente vale 1", () => {
    const rounds = groupByRound([task("1", { round: 2 }), task("2"), task("3", { round: 1 }), task("4", { round: 3 })]);
    assert.deepEqual(
      rounds.map((r) => [r.round, r.tasks.map((t) => t.id)]),
      [
        [1, ["2", "3"]],
        [2, ["1"]],
        [3, ["4"]],
      ],
    );
    assert.deepEqual(groupByRound([]), []);
  });
});

describe("Serve a te", () => {
  it("una board senza nulla in attesa è vuota", () => {
    assert.deepEqual(needsYou(board({ tasks: [task("1"), task("2", { status: "done" })] })), []);
  });

  it("ordine: proposte, impostazioni, domande, attenzione, falliti", () => {
    const b = board({
      tasks: [
        task("fail", { status: "failed" }),
        task("att", { status: "running", attention: true }),
        task("ask", { status: "running", question: { id: "q" } }),
      ],
      proposals: [{ id: "p1", tasks: [] }],
      settings_proposals: [{ id: "s1" }],
    });
    assert.deepEqual(needsYou(b).map(needsYouKey), [
      "proposal:p1",
      "settings:s1",
      "attention:ask",
      "attention:att",
      "failed:fail",
    ]);
    assert.equal(needsYou(b)[2].question, true);
    assert.equal(needsYou(b)[3].question, false);
  });

  it("proposta con goal nuovo o esistente; correzione solo se viene da una review", () => {
    const b = board({
      goals: [goal("a", { round: 2 })],
      proposals: [
        { id: "nuovo", goal: { id: "n", title: "Nuovo goal" }, tasks: [] },
        { id: "aggiunta", goal_id: "a", tasks: [] },
        { id: "correzione", goal_id: "a", source_task: "rev", tasks: [] },
        { id: "sciolta", tasks: [] },
      ],
    });
    const items = needsYou(b);
    assert.deepEqual(
      items.map((i) => [i.goal, i.round]),
      [
        ["Nuovo goal", null],
        ["Goal a", null],
        ["Goal a", 3],
        [null, null],
      ],
    );
  });

  it("falliti: motivo, goal e task in coda che blocca", () => {
    const b = board({
      goals: [goal("a")],
      tasks: [
        task("rotto", { status: "failed", goal: "a", fail_reason: "stopped" }),
        task("b1", { blocked_by: ["rotto"] }),
        task("b2", { blocked_by: ["altro"] }),
        task("b3", { status: "done", blocked_by: ["rotto"] }),
      ],
    });
    const [item] = needsYou(b);
    assert.equal(item.kind, "failed");
    assert.equal(item.goal, "Goal a");
    assert.equal(item.reason, "Fermato dall'utente");
    assert.deepEqual(item.blocking, ["Task b1"]);
  });

  it("una board vecchia senza `goals` non rompe", () => {
    const b = board();
    delete b.goals;
    b.proposals = [{ id: "p", goal_id: "x", source_task: "r", tasks: [] }];
    assert.deepEqual(
      needsYou(b).map((i) => [i.goal, i.round]),
      [["x", null]],
    );
  });
});

describe("preferenze del filtro per goal", () => {
  const memory = () => {
    const data = new Map();
    return { getItem: (k) => data.get(k) ?? null, setItem: (k, v) => data.set(k, v) };
  };

  it("di default nessun filtro e nessun raggruppamento", () => {
    const p = defaultPrefs();
    assert.equal(p.goal, null);
    assert.equal(p.list.groupByGoal, false);
  });

  it("si ricorda per workspace", () => {
    const m = memory();
    const p = defaultPrefs();
    writePrefs(m, "w1", { ...p, goal: "a", list: { ...p.list, groupByGoal: true } });
    assert.equal(readPrefs(m, "w1").goal, "a");
    assert.equal(readPrefs(m, "w1").list.groupByGoal, true);
    assert.equal(readPrefs(m, "w2").goal, null);
  });

  it("valori malformati tornano al default", () => {
    const m = memory();
    m.setItem("cli-companion.tasks-manager.prefs", JSON.stringify({ w: { goal: 7, list: { groupByGoal: "sì" } } }));
    const p = readPrefs(m, "w");
    assert.equal(p.goal, null);
    assert.equal(p.list.groupByGoal, false);
  });
});
