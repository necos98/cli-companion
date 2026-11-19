import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  FOLLOW_UP_WINDOW_DAYS,
  closedRecently,
  followUpEntries,
  followUpRequest,
  followUpStateLabel,
  openFollowUpGroups,
} from "../src/lib/followUps.ts";
import { needsYou, needsYouKey } from "../src/lib/needsYou.ts";

const DAY = 86_400_000;
const NOW = 100 * DAY;

const entry = (text, state = "open", source_task = "a") => ({ text, source_task, state });
const goal = (id, over = {}) => ({
  id,
  title: `Goal ${id}`,
  round: 1,
  waiting: [],
  follow_ups: [],
  ...over,
});
const closed = (id, daysAgo, follow_ups) => goal(id, { closed_at: NOW - daysAgo * DAY, follow_ups });
const board = (goals) => ({ tasks: [], proposals: [], settings_proposals: [], goals });

describe("follow-up dei goal chiusi", () => {
  it("finestra di 14 giorni, estremo incluso", () => {
    assert.equal(FOLLOW_UP_WINDOW_DAYS, 14);
    assert.equal(closedRecently({ closed_at: NOW - 14 * DAY }, NOW), true);
    assert.equal(closedRecently({ closed_at: NOW - 14 * DAY - 1 }, NOW), false);
    assert.equal(closedRecently({}, NOW), false, "un goal aperto non ha chiusura");
  });

  it("solo i follow-up aperti dei goal chiusi di recente, un gruppo per goal", () => {
    const goals = [
      closed("old", 15, [entry("vecchio")]),
      closed("done", 2, [entry("deciso", "dismissed"), entry("mandato", "promoted")]),
      closed("fresh", 1, [entry("uno"), entry("due", "dismissed"), entry("tre")]),
      goal("running", { follow_ups: [entry("mai su un goal aperto")] }),
      closed("empty", 3, []),
    ];
    const groups = openFollowUpGroups(goals, NOW);
    assert.deepEqual(
      groups.map((g) => g.goal.id),
      ["fresh"],
    );
    assert.deepEqual(
      groups[0].entries.map((e) => [e.index, e.followUp.text]),
      [
        [0, "uno"],
        [2, "tre"],
      ],
      "l'indice è la posizione nel goal, non nell'elenco filtrato",
    );
  });

  it("il goal chiuso più di recente per primo", () => {
    const goals = [closed("a", 10, [entry("x")]), closed("b", 1, [entry("y")]), closed("c", 5, [entry("z")])];
    assert.deepEqual(
      openFollowUpGroups(goals, NOW).map((g) => g.goal.id),
      ["b", "c", "a"],
    );
  });

  it("un goal senza il campo (board vecchia) non rompe nulla", () => {
    const old = { id: "g", title: "G", closed_at: NOW - DAY };
    assert.deepEqual(openFollowUpGroups([old], NOW), []);
    assert.deepEqual(followUpEntries(old), []);
  });

  it("le voci di un goal con indice, tutte o per stato", () => {
    const g = closed("g", 1, [entry("a"), entry("b", "promoted"), entry("c", "dismissed")]);
    assert.deepEqual(
      followUpEntries(g).map((e) => e.index),
      [0, 1, 2],
    );
    assert.deepEqual(
      followUpEntries(g, "promoted").map((e) => e.followUp.text),
      ["b"],
    );
  });

  it("etichette di stato", () => {
    assert.equal(followUpStateLabel("open"), "Aperto");
    assert.equal(followUpStateLabel("dismissed"), "Scartato");
    assert.equal(followUpStateLabel("promoted"), "Mandato all'Intake");
  });

  it("la richiesta all'Intake porta il testo e il goal d'origine (titolo e id)", () => {
    const text = followUpRequest(
      { id: "export", title: "Export orders" },
      entry("add pagination", "open", "export-csv"),
    );
    assert.match(text, /"Export orders" \(id: export\)/);
    assert.match(text, /task export-csv/);
    assert.match(text, /^add pagination$/m);
    assert.match(text, /get_goal \("export"\)/);
  });
});

describe("follow-up in Serve a te", () => {
  it("compaiono dopo i falliti solo se si passa l'istante", () => {
    const b = board([closed("g", 1, [entry("x")])]);
    assert.deepEqual(needsYou(b), []);
    const items = needsYou(b, NOW);
    assert.deepEqual(
      items.map((i) => i.kind),
      ["follow_ups"],
    );
    assert.equal(needsYouKey(items[0]), "follow_ups:g");
    assert.equal(items[0].entries.length, 1);
  });

  it("scartata o mandata l'ultima voce, il goal esce dal pannello", () => {
    const b = board([closed("g", 1, [entry("x", "dismissed"), entry("y", "promoted")])]);
    assert.deepEqual(needsYou(b, NOW), []);
  });
});
