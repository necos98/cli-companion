import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { deliveryAction, deliveryTask, goalPhaseLabel, goalWaitingLabels } from "../src/lib/goals.ts";
import { deliverBotChoices, deliverBotId, goalBranchName, prLinkLabel } from "../src/lib/goalDelivery.ts";
import { needsYou, needsYouKey } from "../src/lib/needsYou.ts";

const goal = (id, over = {}) => ({
  id,
  title: `Goal ${id}`,
  request: "r",
  done_when: [],
  decisions: [],
  round: 1,
  created_at: 1,
  phase: "ready",
  waiting: [],
  last_review: "none",
  auto_deliver: false,
  tasks: { done: 0, total: 0 },
  ...over,
});
const task = (id, over = {}) => ({
  id,
  title: `Task ${id}`,
  status: "queued",
  waiting_on: [],
  blocked_by: [],
  attention: false,
  ...over,
});
const bot = (id, over = {}) => ({ id, name: id.toUpperCase(), enabled: true, kinds: ["work"], ...over });

describe("consegna del goal", () => {
  it("etichette di fase e attesa", () => {
    assert.equal(goalPhaseLabel({ phase: "delivering" }), "In consegna");
    assert.deepEqual(goalWaitingLabels({ waiting: ["deliver"] }), ["Da consegnare"]);
  });

  it("azione: se aspetta la consegna o è pronto, 'comunque' se la review ha problemi", () => {
    assert.equal(deliveryAction(goal("a", { phase: "executing" })), null);
    assert.equal(deliveryAction(goal("a", { phase: "delivering", last_review: "issues" })), null);
    assert.equal(deliveryAction(goal("a")), "Consegna");
    assert.equal(deliveryAction(goal("a", { last_review: "issues" })), "Consegna comunque");
    assert.equal(deliveryAction(goal("a", { waiting: ["deliver"], last_review: "passed" })), "Consegna");
    assert.equal(deliveryAction(goal("a", { waiting: ["deliver"], last_review: "none" })), "Consegna");
    assert.equal(deliveryAction(goal("a", { waiting: ["deliver"], last_review: "issues" })), "Consegna comunque");
  });

  it("task di consegna: del goal, kind deliver, il più recente", () => {
    const tasks = [
      task("w", { goal: "a", kind: "work" }),
      task("d0", { goal: "b", kind: "deliver" }),
      task("d1", { goal: "a", kind: "deliver" }),
      task("d2", { goal: "a", kind: "deliver" }),
    ];
    assert.equal(deliveryTask(tasks, "a").id, "d2");
    assert.equal(deliveryTask(tasks, "c"), null);
  });

  it("Bot: predefinito general, solo accesi con work, il scelto resta", () => {
    assert.equal(deliverBotId({}), "general");
    assert.equal(deliverBotId({ deliver_bot: "x" }), "x");
    const bots = [bot("general"), bot("rev", { kinds: ["review"] }), bot("off", { enabled: false }), bot("w2")];
    assert.deepEqual(
      deliverBotChoices(bots, "general").map((c) => c.id),
      ["general", "w2"],
    );
    assert.deepEqual(
      deliverBotChoices(bots, "off").map((c) => c.id),
      ["off", "general", "w2"],
    );
    assert.deepEqual(deliverBotChoices([], "gone"), [{ id: "gone", label: "gone" }]);
  });

  it("branch e link delle PR", () => {
    assert.equal(goalBranchName("abc"), "goal/abc");
    assert.equal(prLinkLabel("https://github.com/o/r/pull/12"), "PR #12");
    assert.equal(prLinkLabel("https://x/y"), "https://x/y");
  });
});

describe("Serve a te: goal da consegnare", () => {
  const board = (over) => ({ tasks: [], proposals: [], settings_proposals: [], goals: [], ...over });

  it("voce deliver con l'etichetta del pulsante; i chiusi non contano", () => {
    const items = needsYou(
      board({
        goals: [
          goal("a", { waiting: ["deliver"] }),
          goal("b", { waiting: ["deliver"], last_review: "issues" }),
          goal("c", { waiting: ["deliver"], closed_at: 5 }),
          goal("d"),
        ],
      }),
    );
    assert.deepEqual(
      items.map((i) => [i.kind, i.label]),
      [
        ["deliver", "Consegna"],
        ["deliver", "Consegna comunque"],
      ],
    );
    assert.equal(needsYouKey(items[0]), "deliver:a");
  });

  it("ordine: proposte, impostazioni, goal pronti, attenzione, falliti", () => {
    const items = needsYou(
      board({
        goals: [goal("a", { waiting: ["deliver"] })],
        proposals: [{ id: "p", tasks: [] }],
        settings_proposals: [{ id: "s" }],
        tasks: [task("f", { status: "failed" }), task("t", { status: "running", attention: true })],
      }),
    );
    assert.deepEqual(
      items.map((i) => i.kind),
      ["proposal", "settings", "deliver", "attention", "failed"],
    );
  });
});
