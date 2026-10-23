import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  INTAKE_LAUNCH_ID,
  applyAutomationUpdate,
  automationTooltip,
  findIntakeTabKey,
  isAutomated,
  isIntakeLaunch,
  persistableTabs,
  statusWord,
  tabBadge,
  truncateReport,
} from "../src/lib/automation.ts";
import { EMPTY_LAYOUT, openTab, openTabBackground } from "../src/lib/layout.ts";

const meta = (extra = {}) => ({
  title: "Fix login",
  status: "starting",
  agentId: "claude-code",
  ...extra,
});

describe("automazione: stati", () => {
  it("parole italiane per ogni stato", () => {
    const expected = {
      starting: "in avvio",
      running: "in corso",
      background: "in background",
      needs_input: "aspetta",
      idle: "fermo",
      completed: "completato",
      failed: "fallito",
      closed: "chiuso",
    };
    for (const [s, text] of Object.entries(expected)) {
      assert.equal(statusWord(s).text, text);
      assert.match(statusWord(s).className, /^text-/);
    }
  });

  it("stato sconosciuto: mostrato com'e'", () => {
    assert.equal(statusWord("boh").text, "boh");
  });
});

describe("automazione: metadati e tooltip", () => {
  it("isAutomated", () => {
    assert.equal(isAutomated({ automation: meta() }), true);
    assert.equal(isAutomated({}), false);
    assert.equal(isAutomated({ automation: undefined }), false);
  });

  it("truncateReport compatta e tronca", () => {
    assert.equal(truncateReport("a\n\n  b"), "a b");
    const t = truncateReport("x".repeat(500), 20);
    assert.equal(t.length, 20);
    assert.ok(t.endsWith("…"));
  });

  it("tooltip con titolo, stato e report", () => {
    const tip = automationTooltip(meta({ status: "running", lastReport: "fatto  tutto" }));
    assert.match(tip, /Fix login/);
    assert.match(tip, /in corso/);
    assert.match(tip, /Ultimo report: fatto tutto/);
  });

  it("tooltip senza report", () => {
    const tip = automationTooltip(meta());
    assert.doesNotMatch(tip, /report/);
  });

  it("applyAutomationUpdate aggiorna stato, report e titolo", () => {
    const next = applyAutomationUpdate(meta(), {
      status: "completed",
      title: "Nuovo",
      last_report: "ok",
    });
    assert.equal(next.status, "completed");
    assert.equal(next.title, "Nuovo");
    assert.equal(next.lastReport, "ok");
  });

  it("applyAutomationUpdate: report assente e titolo vuoto non cancellano", () => {
    const m = meta({ lastReport: "vecchio" });
    const next = applyAutomationUpdate(m, { status: "idle", title: " ", last_report: null });
    assert.equal(next.title, "Fix login");
    assert.equal(next.lastReport, "vecchio");
    assert.equal(next.status, "idle");
  });

  it("applyAutomationUpdate: nessuna modifica = stessa istanza", () => {
    const m = meta();
    assert.equal(applyAutomationUpdate(m, { status: "starting", title: "Fix login" }), m);
  });
});

describe("automazione: persistenza", () => {
  it("i tab automatici non vengono salvati", () => {
    const tabs = [{ key: "a" }, { key: "w", automation: meta() }, { key: "b" }];
    assert.deepEqual(
      persistableTabs(tabs).map((t) => t.key),
      ["a", "b"],
    );
  });
});

describe("layout: apertura in secondo piano", () => {
  it("non cambia gruppo, vista ne' pannello attivo", () => {
    let l = openTab(EMPTY_LAYOUT, "w1", "a");
    l = openTab(l, "w2", "b");
    const next = openTabBackground(l, "w1", "auto");
    assert.equal(next.activeWorkspaceId, "w2");
    const g1 = next.groups.find((g) => g.workspaceId === "w1");
    assert.deepEqual(g1.order, ["a", "auto"]);
    assert.equal(g1.activeTabId, "a");
  });

  it("gruppo nuovo in coda, senza prendere il posto di quello attivo", () => {
    const l = openTab(EMPTY_LAYOUT, "w1", "a");
    const next = openTabBackground(l, "w2", "auto");
    assert.equal(next.activeWorkspaceId, "w1");
    assert.deepEqual(
      next.groups.map((g) => g.workspaceId),
      ["w1", "w2"],
    );
  });

  it("senza gruppi diventa quello attivo; maximize e vista globale restano", () => {
    assert.equal(openTabBackground(EMPTY_LAYOUT, "w1", "auto").activeWorkspaceId, "w1");
    const l = { ...openTab(EMPTY_LAYOUT, "w1", "a"), activeView: "global" };
    assert.equal(openTabBackground(l, "w1", "auto").activeView, "global");
  });

  it("tab gia' presente: stessa istanza", () => {
    const l = openTab(EMPTY_LAYOUT, "w1", "a");
    assert.equal(openTabBackground(l, "w1", "a"), l);
  });
});

describe("automazione: badge del tab", () => {
  it("i worker hanno AUTO", () => {
    const b = tabBadge({ automation: meta() });
    assert.deepEqual(b, { text: "AUTO", title: "Sessione automatica", kind: "auto" });
  });

  it("i tab avviati dal preset Task Intake hanno INTAKE", () => {
    assert.equal(INTAKE_LAUNCH_ID, "builtin:preset:task-intake");
    const b = tabBadge({ launchId: INTAKE_LAUNCH_ID });
    assert.equal(b.text, "INTAKE");
    assert.equal(b.kind, "intake");
  });

  it("gli altri tab non hanno badge", () => {
    assert.equal(tabBadge({}), null);
    assert.equal(tabBadge({ launchId: "builtin:claude-code" }), null);
    assert.equal(tabBadge({ launchId: "builtin:preset:altro" }), null);
  });

  it("un tab Intake non e' automatico: resta persistibile", () => {
    const tab = { key: "t", launchId: INTAKE_LAUNCH_ID };
    assert.equal(isAutomated(tab), false);
    assert.equal(persistableTabs([tab]).length, 1);
    assert.equal(isIntakeLaunch(INTAKE_LAUNCH_ID), true);
    assert.equal(isIntakeLaunch(undefined), false);
  });
});

describe("automazione: tab Intake aperto", () => {
  const tabs = [
    { key: "a", workspace: { id: "w1" } },
    { key: "b", workspace: { id: "w2" }, launchId: INTAKE_LAUNCH_ID },
    { key: "c", workspace: { id: "w1" }, launchId: INTAKE_LAUNCH_ID },
    { key: "d", workspace: { id: "w1" }, launchId: INTAKE_LAUNCH_ID },
  ];
  it("trova il primo tab Intake del workspace", () => {
    assert.equal(findIntakeTabKey(tabs, "w1"), "c");
    assert.equal(findIntakeTabKey(tabs, "w2"), "b");
  });
  it("nessun tab Intake: null", () => {
    assert.equal(findIntakeTabKey(tabs, "w3"), null);
    assert.equal(findIntakeTabKey([tabs[0]], "w1"), null);
  });
});
