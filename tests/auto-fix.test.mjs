import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  MAX_AUTO_FIX_ROUNDS,
  ROUND_LIMIT_NOTE,
  autoFixChoices,
  autoFixHoldText,
  autoFixLabel,
  autoFixUsageLabel,
  isAutomaticRound,
  proposalAutoFixNote,
  roundAcceptanceLabel,
} from "../src/lib/autoFix.ts";
import { goalWaitingLabels } from "../src/lib/goals.ts";

const goal = (id, over = {}) => ({ id, round: 1, auto_fix: { max_rounds: 0 }, auto_rounds: [], ...over });
const correction = (over = {}) => ({ id: "p", source_task: "rev", goal_id: "g", ...over });

describe("correzione automatica del goal", () => {
  it("scelte: da no al tetto, con il singolare", () => {
    assert.equal(MAX_AUTO_FIX_ROUNDS, 3);
    assert.deepEqual(autoFixChoices(), [
      { value: 0, label: "no" },
      { value: 1, label: "1 giro" },
      { value: 2, label: "2 giri" },
      { value: 3, label: "3 giri" },
    ]);
    assert.equal(autoFixLabel(-1), "no");
  });

  it("giri usati sul tetto, null se spenta", () => {
    assert.equal(autoFixUsageLabel(goal("g")), null);
    assert.equal(autoFixUsageLabel(goal("g", { auto_fix: { max_rounds: 2 }, auto_rounds: [2] })), "1/2");
    // Il tetto abbassato dopo i giri usati non supera il totale.
    assert.equal(autoFixUsageLabel(goal("g", { auto_fix: { max_rounds: 1 }, auto_rounds: [2, 3] })), "1/1");
  });

  it("chi ha accettato ogni giro", () => {
    const g = goal("g", { auto_rounds: [2] });
    assert.equal(isAutomaticRound(g, 2), true);
    assert.equal(isAutomaticRound(g, 3), false);
    assert.equal(roundAcceptanceLabel(g, 1), null);
    assert.equal(roundAcceptanceLabel(g, 2), "accettato in automatico");
    assert.equal(roundAcceptanceLabel(g, 3), "accettato da te");
  });

  it("motivi per cui una correzione non entra da sola", () => {
    assert.equal(autoFixHoldText({ reason: "not_from_review" }), "non viene dalla review del goal");
    assert.match(autoFixHoldText({ reason: "kind", task: "x", kind: "plan" }), /tipo plan/);
    assert.match(autoFixHoldText({ reason: "repo", task: "x", repo: "api" }), /in api che il goal non usa/);
    assert.match(autoFixHoldText({ reason: "repo", task: "x", repo: "." }), /in un repository/);
    assert.match(autoFixHoldText({ reason: "bot", task: "x", bot: "git-operator" }), /git-operator, un Bot spento/);
    assert.equal(autoFixHoldText({ reason: "invalid", error: "boom" }), "validazione fallita: boom");
  });

  it("nota della card: tetto raggiunto, motivo, o niente", () => {
    const capped = goal("g", { round: 3, auto_fix: { max_rounds: 2 } });
    const note = proposalAutoFixNote(correction(), [capped]);
    assert.ok(note.startsWith(ROUND_LIMIT_NOTE), note);
    assert.match(note.toLowerCase(), /giri automatici finiti \(2\/2\)/);
    // Giri ancora disponibili, correzione automatica spenta, proposta dell'Intake, goal sconosciuto: niente.
    assert.equal(proposalAutoFixNote(correction(), [goal("g", { round: 2, auto_fix: { max_rounds: 2 } })]), null);
    assert.equal(proposalAutoFixNote(correction(), [goal("g", { round: 5 })]), null);
    assert.equal(proposalAutoFixNote(correction({ source_task: undefined }), [capped]), null);
    assert.equal(proposalAutoFixNote(correction(), []), null);
    // Il motivo scritto dal backend vince.
    const held = correction({ auto_fix_hold: { reason: "invalid", error: "x" } });
    assert.equal(proposalAutoFixNote(held, []), "Non accettata in automatico: validazione fallita: x");
  });

  it("attesa del goal: giri automatici finiti", () => {
    assert.deepEqual(goalWaitingLabels({ waiting: ["round_limit"] }), [
      "Giri automatici finiti: una correzione da decidere",
    ]);
  });
});
