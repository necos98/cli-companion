import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  hubCoversAgentEvent,
  hubCoversTaskNotice,
  hubNotifies,
  shouldShowHubNotice,
  soundOf,
  threadIsInView,
} from "../src/lib/hubNotify.ts";

const target = { channel: "ws-1", thread_id: "t1" };
const watching = { focused: true, hubShown: true, channel: "ws-1", threadId: "t1" };
const hub = (notifications, enabled = true) => ({ enabled, settings: { notifications } });

describe("visibilita' del thread", () => {
  it("il thread aperto in primo piano e' in vista: nessuna notifica", () => {
    assert.equal(threadIsInView(watching, target), true);
    assert.equal(shouldShowHubNotice(watching, target), false);
  });

  it("finestra non in primo piano: si mostra anche sul thread aperto", () => {
    assert.equal(shouldShowHubNotice({ ...watching, focused: false }, target), true);
  });

  it("nel mondo Terminali si mostra", () => {
    assert.equal(shouldShowHubNotice({ ...watching, hubShown: false }, target), true);
  });

  it("un altro thread o un altro canale: si mostra", () => {
    assert.equal(shouldShowHubNotice({ ...watching, threadId: "t2" }, target), true);
    assert.equal(shouldShowHubNotice({ ...watching, threadId: null }, target), true);
    assert.equal(shouldShowHubNotice({ ...watching, channel: "general" }, target), true);
  });
});

describe("suono", () => {
  it("attention resta attention, il resto e' il ding", () => {
    assert.equal(soundOf("attention"), "attention");
    assert.equal(soundOf("ding"), "ding");
  });
});

describe("doppioni con task-notify", () => {
  it("con le notifiche dell'HUB attive copre fallito, attenzione e domanda", () => {
    for (const kind of ["failed", "attention", "question"]) assert.equal(hubCoversTaskNotice(hub(true), kind), true);
  });

  it("un tipo sconosciuto non e' coperto: task-notify lo mostra", () => {
    assert.equal(hubCoversTaskNotice(hub(true), "other"), false);
    assert.equal(hubCoversTaskNotice(hub(true), "fixes"), false);
  });

  it("interruttore spento: task-notify torna com'era", () => {
    for (const kind of ["failed", "attention", "question"]) assert.equal(hubCoversTaskNotice(hub(false), kind), false);
  });

  it("HUB spento o impostazioni non lette: task-notify torna com'era", () => {
    assert.equal(hubNotifies(hub(true, false)), false);
    assert.equal(hubNotifies({ enabled: true, settings: null }), false);
    assert.equal(hubCoversTaskNotice({ enabled: false, settings: null }, "failed"), false);
  });
});

describe("terminali silenziati dall'HUB", () => {
  it("HUB e notifiche accesi: gli eventi dei coding agent non notificano", () => {
    assert.equal(hubCoversAgentEvent(hub(true)), true);
  });

  it("notifiche spente: gli eventi dei coding agent notificano come prima", () => {
    assert.equal(hubCoversAgentEvent(hub(false)), false);
  });

  it("HUB spento: gli eventi dei coding agent notificano come prima", () => {
    assert.equal(hubCoversAgentEvent(hub(true, false)), false);
  });

  it("impostazioni non ancora lette: gli eventi dei coding agent notificano come prima", () => {
    assert.equal(hubCoversAgentEvent({ enabled: true, settings: null }), false);
  });
});
