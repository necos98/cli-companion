import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  GOAL_NOTE_MAX,
  closeBlocked,
  decisionSourceLabel,
  goalCloseImpact,
  goalNoteError,
} from "../src/lib/goalDetail.ts";
import { goalDependencyPool } from "../src/lib/taskForm.ts";

const task = (id, over = {}) => ({ id, title: `Task ${id}`, status: "queued", depends_on: [], ...over });

describe("nota del goal", () => {
  it("accetta da 1 a 500 caratteri, spazi ai bordi esclusi", () => {
    assert.equal(goalNoteError("ok"), undefined);
    assert.equal(goalNoteError("a".repeat(GOAL_NOTE_MAX)), undefined);
    assert.ok(goalNoteError("a".repeat(GOAL_NOTE_MAX + 1)));
    assert.ok(goalNoteError("   "));
    assert.equal(goalNoteError(`  ${"a".repeat(GOAL_NOTE_MAX)}  `), undefined);
  });

  it("conta i caratteri, non le unità UTF-16", () => {
    assert.equal(goalNoteError("😀".repeat(GOAL_NOTE_MAX)), undefined);
  });

  it("fonte delle decisioni in italiano", () => {
    assert.equal(decisionSourceLabel("user"), "Tu");
    assert.equal(decisionSourceLabel("intake"), "Intake");
    assert.equal(decisionSourceLabel("app"), "App");
  });
});

describe("chiusura del goal", () => {
  const board = {
    tasks: [
      task("a", { goal: "g", status: "queued" }),
      task("b", { goal: "g", status: "failed" }),
      task("c", { goal: "g", status: "done" }),
      task("d", { goal: "altro", status: "queued" }),
      task("e", { status: "queued" }),
    ],
    proposals: [
      { id: "p1", goal_id: "g", tasks: [{ title: "Uno" }, { title: "Due" }] },
      { id: "p2", goal_id: "altro", tasks: [{ title: "X" }] },
      { id: "p3", tasks: [{ title: "Y" }] },
    ],
  };

  it("elenca i task in coda e falliti e le proposte pendenti del goal", () => {
    const impact = goalCloseImpact(board, "g");
    assert.deepEqual(
      impact.removed.map((t) => t.id),
      ["a", "b"],
    );
    assert.deepEqual(impact.proposals, ["Uno, Due"]);
    assert.equal(closeBlocked(impact), false);
  });

  it("un task in corso blocca la chiusura", () => {
    const running = { ...board, tasks: [...board.tasks, task("r", { goal: "g", status: "running" })] };
    const impact = goalCloseImpact(running, "g");
    assert.deepEqual(
      impact.running.map((t) => t.id),
      ["r"],
    );
    assert.equal(closeBlocked(impact), true);
  });
});

describe("dipendenze selezionabili per goal", () => {
  const goals = [{ id: "g" }, { id: "h" }, { id: "x", closed_at: 5 }];
  const tasks = [task("a", { goal: "g" }), task("b", { goal: "h" }), task("c", { goal: "x" }), task("d")];

  it("un task di un goal dipende solo da task dello stesso goal", () => {
    assert.deepEqual(
      goalDependencyPool(tasks, goals, "g").map((t) => t.id),
      ["a"],
    );
  });

  it("un task sciolto non dipende da task di un goal aperto", () => {
    assert.deepEqual(
      goalDependencyPool(tasks, goals, null).map((t) => t.id),
      ["c", "d"],
    );
  });
});
