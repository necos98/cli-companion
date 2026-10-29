import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  ACTIVITY_LABELS,
  DESKS_PER_ROW,
  IDLE_MAX_MS,
  IDLE_MIN_MS,
  MIN_DESKS,
  ROOM_WIDTH,
  assignDesks,
  botActivity,
  botPlacement,
  officeRooms,
  officeSession,
  planIdle,
  restingGoals,
  roomLayout,
} from "../src/lib/office.ts";

/** PRNG deterministico (mulberry32), valori in [0,1). */
function seeded(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// ---------------------------------------------------------------- botActivity

describe("botActivity", () => {
  const act = (tab) => botActivity({ busy: false, ...tab });

  it("lo stato del worker noto vince su agente e shell", () => {
    const agent = { phase: "idle" };
    const cases = {
      running: "working",
      starting: "working",
      needs_input: "attention",
      idle: "attention",
      background: "background",
      completed: "completed",
      failed: "failed",
    };
    for (const [status, expected] of Object.entries(cases)) {
      assert.equal(act({ automation: { status }, agent, busy: false }), expected, status);
    }
  });

  it("worker closed o sconosciuto: ricade su agente e shell", () => {
    for (const status of ["closed", "boh", "toString", ""]) {
      assert.equal(act({ automation: { status } }), "idle", status);
      assert.equal(act({ automation: { status }, busy: true }), "working", status);
      assert.equal(act({ automation: { status }, agent: { phase: "running" } }), "working", status);
    }
  });

  it("fasi dell'agente", () => {
    assert.equal(act({ agent: { phase: "running" } }), "working");
    assert.equal(act({ agent: { phase: "needs-input" } }), "attention");
    assert.equal(act({ agent: { phase: "idle" } }), "idle");
    assert.equal(act({ agent: { phase: "finished" } }), "idle");
    // La fase dell'agente vince sulla shell occupata.
    assert.equal(act({ agent: { phase: "idle" }, busy: true }), "idle");
  });

  it("agente terminato: conta solo la shell", () => {
    for (const phase of ["running", "needs-input", "idle", "finished"]) {
      assert.equal(act({ agent: { phase, ended: true }, busy: true }), "working", phase);
      assert.equal(act({ agent: { phase, ended: true }, busy: false }), "idle", phase);
    }
  });

  it("senza agente: shell occupata o no", () => {
    assert.equal(act({ busy: true }), "working");
    assert.equal(act({ busy: false }), "idle");
  });
});

describe("ACTIVITY_LABELS", () => {
  it("ha un'etichetta per ogni attivita'", () => {
    const all = ["working", "idle", "attention", "background", "completed", "failed"];
    assert.deepEqual(Object.keys(ACTIVITY_LABELS).sort(), [...all].sort());
    for (const a of all) assert.ok(ACTIVITY_LABELS[a].length > 0, a);
    assert.equal(ACTIVITY_LABELS.working, "Al lavoro");
    assert.equal(ACTIVITY_LABELS.idle, "In pausa");
  });
});

// ---------------------------------------------------------------- officeSession

describe("officeSession", () => {
  const base = { key: "t1", bot: "Bolt", workspace: { id: "w1", name: "App" } };

  it("copia i campi e calcola l'attivita'", () => {
    const s = officeSession({ ...base, agent: { phase: "running" } }, false, "Bolt");
    assert.deepEqual(s, {
      key: "t1",
      bot: "Bolt",
      workspaceId: "w1",
      workspaceName: "App",
      label: "Bolt",
      activity: "working",
      intake: false,
    });
    assert.equal("taskTitle" in s, false);
  });

  it("riconosce i tab del Task Intake", async () => {
    const { INTAKE_LAUNCH_ID } = await import("../src/lib/automation.ts");
    const s = officeSession({ ...base, launchId: INTAKE_LAUNCH_ID }, true, "x");
    assert.equal(s.intake, true);
    assert.equal(officeSession({ ...base, launchId: "altro" }, true, "x").intake, false);
  });

  it("taskTitle solo per i tab dei worker", () => {
    const w = officeSession(
      { ...base, automation: { status: "completed", title: "Fix login" } },
      false,
      "Fix login",
    );
    assert.equal(w.taskTitle, "Fix login");
    assert.equal(w.activity, "completed");
    assert.equal(officeSession(base, false, "Bolt").taskTitle, undefined);
  });
});

// ---------------------------------------------------------------- assignDesks

const ses = (key, workspaceId = "w1") => ({ key, workspaceId });

describe("assignDesks", () => {
  it("assegna in ordine le scrivanie, una numerazione per workspace", () => {
    const d = assignDesks(new Map(), [ses("a"), ses("b"), ses("x", "w2"), ses("c"), ses("y", "w2")]);
    assert.deepEqual([...d], [["a", 0], ["b", 1], ["x", 0], ["c", 2], ["y", 1]]);
  });

  it("aprire o chiudere altri tab non sposta nessuno", () => {
    const d1 = assignDesks(new Map(), [ses("a"), ses("b"), ses("c")]);
    const d2 = assignDesks(d1, [ses("c"), ses("a"), ses("b"), ses("d")]);
    for (const k of ["a", "b", "c"]) assert.equal(d2.get(k), d1.get(k), k);
    assert.equal(d2.get("d"), 3);
    const d3 = assignDesks(d2, [ses("a"), ses("c")]);
    assert.equal(d3.get("a"), 0);
    assert.equal(d3.get("c"), 2);
    assert.equal(d3.has("b"), false);
  });

  it("i nuovi riusano la scrivania libera piu' bassa", () => {
    const d1 = assignDesks(new Map(), [ses("a"), ses("b"), ses("c")]);
    const d2 = assignDesks(d1, [ses("a"), ses("c")]);
    const d3 = assignDesks(d2, [ses("a"), ses("c"), ses("n1"), ses("n2")]);
    assert.equal(d3.get("n1"), 1);
    assert.equal(d3.get("n2"), 3);
  });

  it("nulla cambiato: stessa istanza", () => {
    const d1 = assignDesks(new Map(), [ses("a"), ses("b")]);
    assert.equal(assignDesks(d1, [ses("b"), ses("a")]), d1);
    const empty = new Map();
    assert.equal(assignDesks(empty, []), empty);
  });

  it("un tab chiuso cambia l'istanza", () => {
    const d1 = assignDesks(new Map(), [ses("a"), ses("b")]);
    assert.notEqual(assignDesks(d1, [ses("a")]), d1);
  });

  it("non muta l'input", () => {
    const prev = new Map([["a", 0]]);
    assignDesks(prev, [ses("a"), ses("b")]);
    assert.deepEqual([...prev], [["a", 0]]);
  });
});

// ---------------------------------------------------------------- officeRooms

const full = (key, workspaceId, name = workspaceId) => ({
  key,
  bot: key,
  workspaceId,
  workspaceName: name,
  label: key,
  activity: "idle",
  intake: false,
});

describe("officeRooms", () => {
  it("segue l'ordine della Sidebar e salta i workspace senza tab", () => {
    const sessions = [full("a", "w2"), full("b", "w1"), full("c", "w2")];
    const desks = assignDesks(new Map(), sessions);
    const rooms = officeRooms(sessions, ["w3", "w1", "w2"], desks);
    assert.deepEqual(rooms.map((r) => r.workspaceId), ["w1", "w2"]);
  });

  it("i workspace sconosciuti vanno in coda nell'ordine di comparsa", () => {
    const sessions = [full("a", "wz"), full("b", "w1"), full("c", "wy")];
    const desks = assignDesks(new Map(), sessions);
    const rooms = officeRooms(sessions, ["w1"], desks);
    assert.deepEqual(rooms.map((r) => r.workspaceId), ["w1", "wz", "wy"]);
  });

  it("nome del workspace e bot ordinati per scrivania", () => {
    const sessions = [full("a", "w1", "App"), full("b", "w1", "App"), full("c", "w1", "App")];
    const desks = new Map([["a", 2], ["b", 0], ["c", 1]]);
    const [room] = officeRooms(sessions, ["w1"], desks);
    assert.equal(room.name, "App");
    assert.deepEqual(room.bots.map((b) => [b.key, b.desk]), [["b", 0], ["c", 1], ["a", 2]]);
  });

  it("deskCount: minimo 4, poi multipli di 4", () => {
    const count = (n, highest = n - 1) => {
      const sessions = Array.from({ length: n }, (_, i) => full(`k${i}`, "w1"));
      const desks = new Map(sessions.map((s, i) => [s.key, i === n - 1 ? highest : i]));
      return officeRooms(sessions, ["w1"], desks)[0].deskCount;
    };
    assert.equal(MIN_DESKS, 4);
    assert.equal(DESKS_PER_ROW, 4);
    assert.equal(count(1), 4);
    assert.equal(count(4), 4);
    assert.equal(count(5), 8);
    assert.equal(count(8), 8);
    assert.equal(count(9), 12);
    // Una scrivania alta (tab chiusi in mezzo) allarga la stanza.
    assert.equal(count(2, 9), 12);
  });

  it("ignora le sessioni senza scrivania", () => {
    const sessions = [full("a", "w1"), full("b", "w2")];
    const rooms = officeRooms(sessions, ["w1", "w2"], new Map([["a", 0]]));
    assert.deepEqual(rooms.map((r) => r.workspaceId), ["w1"]);
  });

  it("nessuna sessione, nessuna stanza", () => {
    assert.deepEqual(officeRooms([], ["w1"], new Map()), []);
  });
});

// ---------------------------------------------------------------- roomLayout

const MIN_GAP = 1.2;

/** Tutti i punti dove un bot puo' stare, con un'etichetta per i messaggi. */
function allPoints(layout) {
  const pts = [];
  for (const s of layout.spots) pts.push([s.id, s]);
  for (const a of layout.chatAreas) for (const s of a.seats) pts.push([s.id, s]);
  for (const s of layout.server) pts.push([s.id, s]);
  layout.desks.forEach((d, i) => {
    pts.push([`desk-seat-${i}`, d.seat]);
    pts.push([`desk-standing-${i}`, d.standing]);
  });
  return pts;
}

describe("roomLayout", () => {
  for (const deskCount of [4, 8, 12, 30]) {
    it(`vincoli geometrici con ${deskCount} scrivanie`, () => {
      const layout = roomLayout(deskCount);
      assert.equal(layout.width, ROOM_WIDTH);
      assert.equal(layout.desks.length, deskCount);
      assert.ok(layout.spots.length >= deskCount);
      assert.ok(layout.chatAreas.length >= 1);
      assert.ok(layout.server.length >= 1);

      const pts = allPoints(layout);
      for (const [id, p] of pts) {
        assert.ok(p.x >= 0 && p.x <= layout.width, `${id} x=${p.x}`);
        assert.ok(p.y >= 0 && p.y <= layout.height, `${id} y=${p.y}`);
      }
      for (let i = 0; i < pts.length; i++) {
        for (let j = i + 1; j < pts.length; j++) {
          const d = Math.hypot(pts[i][1].x - pts[j][1].x, pts[i][1].y - pts[j][1].y);
          assert.ok(d >= MIN_GAP, `${pts[i][0]} e ${pts[j][0]} a ${d.toFixed(2)}u`);
        }
      }

      const ids = pts.filter(([id]) => !id.startsWith("desk-")).map(([id]) => id);
      assert.equal(new Set(ids).size, ids.length, "id univoci");
      for (const a of layout.chatAreas) assert.equal(a.seats.length, 3);
    });

    it(`scrivanie e arredi dentro la stanza con ${deskCount} scrivanie`, () => {
      const layout = roomLayout(deskCount);
      for (const r of [...layout.desks, ...layout.furniture]) {
        assert.ok(r.x >= 0 && r.y >= 0, JSON.stringify(r));
        assert.ok(r.x + r.w <= layout.width && r.y + r.h <= layout.height, JSON.stringify(r));
      }
    });
  }

  it("e' deterministica e gli id restano stabili al crescere della stanza", () => {
    assert.deepEqual(roomLayout(8), roomLayout(8));
    const ids = (n) => roomLayout(n).spots.map((s) => s.id);
    assert.deepEqual(ids(8).slice(0, ids(4).length), ids(4));
    assert.deepEqual(roomLayout(8).desks.slice(0, 4), roomLayout(4).desks);
  });

  it("la stanza cresce con le scrivanie", () => {
    assert.ok(roomLayout(8).height > roomLayout(4).height);
    assert.ok(roomLayout(12).chatAreas.length > roomLayout(4).chatAreas.length);
  });

  it("i posti di ogni area guardano uno verso l'altro", () => {
    for (const a of roomLayout(8).chatAreas) {
      assert.equal(a.seats[0].facing, 1);
      assert.equal(a.seats[1].facing, -1);
      assert.ok(a.seats[0].x < a.seats[1].x);
    }
  });
});

// ---------------------------------------------------------------- planIdle

const KEYS = ["a", "b", "c", "d", "e", "f"];

/** Un tick di planIdle con generatore e orologio dati. */
const tick = (layout, idle, goals, now, random) => planIdle({ layout, idle, goals, now, random });

/** Invarianti di un risultato di planIdle. */
function checkPlan(layout, idle, goals, now) {
  const seen = new Set();
  const groups = new Map();
  for (const [key, g] of goals) {
    assert.ok(idle.includes(key), `${key} non e' in idle ma ha una meta`);
    assert.ok(!seen.has(g.spotId), `due bot su ${g.spotId}`);
    seen.add(g.spotId);
    assert.ok(g.until > now);
    if (g.chat) {
      assert.equal(g.kind, "chat");
      groups.set(g.chat, [...(groups.get(g.chat) ?? []), g]);
    } else {
      assert.ok(layout.spots.some((s) => s.id === g.spotId), g.spotId);
    }
  }
  for (const [chat, members] of groups) {
    const area = layout.chatAreas.find((a) => a.id === chat);
    assert.ok(area, chat);
    assert.ok(members.length >= 2 && members.length <= 3, `gruppo ${chat} di ${members.length}`);
    assert.equal(new Set(members.map((m) => m.until)).size, 1, "stesso until");
    for (const m of members) assert.ok(area.seats.some((s) => s.id === m.spotId));
  }
}

describe("planIdle", () => {
  const layout = roomLayout(8);

  it("e' deterministico con lo stesso generatore", () => {
    const run = () => {
      const random = seeded(7);
      let goals = new Map();
      const trace = [];
      for (let t = 0; t < 40; t++) {
        goals = tick(layout, KEYS, goals, t * 1000, random);
        trace.push([...goals]);
      }
      return trace;
    };
    assert.deepEqual(run(), run());
  });

  it("assegna una meta a ogni bot libero e durate nei limiti", () => {
    const goals = tick(layout, KEYS, new Map(), 1000, seeded(1));
    assert.equal(goals.size, KEYS.length);
    for (const g of goals.values()) {
      assert.ok(g.until - 1000 >= IDLE_MIN_MS && g.until - 1000 <= IDLE_MAX_MS, String(g.until));
    }
  });

  it("durate estreme del generatore", () => {
    const hi = tick(layout, ["a"], new Map(), 0, () => 0.999999);
    assert.equal([...hi.values()][0].until, IDLE_MAX_MS);
    const lo = tick(layout, ["a"], new Map(), 0, () => 0);
    assert.equal([...lo.values()][0].until, IDLE_MIN_MS);
  });

  it("un bot solo non chiacchiera mai", () => {
    for (let seed = 0; seed < 50; seed++) {
      const goals = tick(layout, ["a"], new Map(), 0, seeded(seed));
      assert.equal([...goals.values()][0].chat, undefined);
    }
  });

  it("forma gruppi da 2 o 3 sulla stessa area, con lo stesso until", () => {
    let chats = 0;
    for (let seed = 0; seed < 200; seed++) {
      const goals = tick(layout, KEYS, new Map(), 0, seeded(seed));
      checkPlan(layout, KEYS, goals, 0);
      chats += [...goals.values()].filter((g) => g.chat).length;
    }
    assert.ok(chats > 0, "almeno una chiacchierata nelle prove");
  });

  it("le chiavi non in idle non ricevono mai mete", () => {
    const idle = ["b", "d"];
    for (let seed = 0; seed < 100; seed++) {
      const goals = tick(layout, idle, new Map(), 0, seeded(seed));
      for (const key of goals.keys()) assert.ok(idle.includes(key), key);
      checkPlan(layout, idle, goals, 0);
    }
  });

  it("i bot che escono dall'idle perdono la meta", () => {
    const goals = tick(layout, KEYS, new Map(), 0, seeded(3));
    const next = tick(layout, ["a", "b"], goals, 1, seeded(4));
    assert.deepEqual([...next.keys()].sort(), ["a", "b"].sort());
    assert.equal(tick(layout, [], goals, 1, seeded(4)).size, 0);
  });

  it("tiene le mete non scadute (stessi oggetti) e sostituisce quelle scadute", () => {
    const goals = tick(layout, ["a", "b"], new Map(), 0, () => 0.99);
    const g = goals.get("a");
    const kept = tick(layout, ["a", "b"], goals, g.until - 1, seeded(1));
    assert.equal(kept.get("a"), g);
    const later = tick(layout, ["a", "b"], goals, g.until, seeded(1));
    assert.notEqual(later.get("a"), g);
    assert.ok(later.get("a").until > g.until);
    checkPlan(layout, ["a", "b"], later, g.until);
  });

  it("la meta scaduta non si ripete subito sullo stesso spot", () => {
    for (let seed = 0; seed < 100; seed++) {
      const first = tick(layout, ["a"], new Map(), 0, () => 0.99);
      const spot = first.get("a").spotId;
      const next = tick(layout, ["a"], first, IDLE_MAX_MS + 1, seeded(seed));
      assert.notEqual(next.get("a").spotId, spot);
    }
  });

  it("se il compagno esce, la chiacchierata si scioglie", () => {
    // Forza una chat di 2: il primo random e' 0 (< CHAT_CHANCE), poi 0.9 (niente trio).
    const forced = [0, 0.9, 0, 0, 0, 0, 0];
    const seq = () => forced.shift() ?? 0;
    const goals = tick(layout, ["a", "b"], new Map(), 0, seq);
    assert.equal(goals.get("a").chat, goals.get("b").chat);
    assert.ok(goals.get("a").chat);
    const next = tick(layout, ["a"], goals, 1, () => 0.99);
    assert.equal(next.size, 1);
    assert.equal(next.get("a").chat, undefined);
    assert.notEqual(next.get("a"), goals.get("a"));
  });

  it("un gruppo da 3 che perde un membro resta in due", () => {
    let found = false;
    for (let seed = 0; seed < 500 && !found; seed++) {
      const goals = tick(layout, KEYS, new Map(), 0, seeded(seed));
      const trio = [...goals].filter(([, g]) => g.chat);
      const byChat = new Map();
      for (const [k, g] of trio) byChat.set(g.chat, [...(byChat.get(g.chat) ?? []), k]);
      const group = [...byChat.values()].find((m) => m.length === 3);
      if (!group) continue;
      found = true;
      const left = KEYS.filter((k) => k !== group[2]);
      const next = tick(layout, left, goals, 1, seeded(seed + 1));
      for (const k of group.slice(0, 2)) assert.equal(next.get(k), goals.get(k));
      checkPlan(layout, left, next, 1);
    }
    assert.ok(found, "nessun trio generato nelle prove");
  });

  it("con nulla di cambiato restituisce la stessa istanza", () => {
    const goals = tick(layout, KEYS, new Map(), 0, seeded(5));
    assert.equal(tick(layout, KEYS, goals, 1, seeded(6)), goals);
    const empty = new Map();
    assert.equal(tick(layout, [], empty, 0, seeded(1)), empty);
  });

  it("se non ci sono spot liberi i bot restano senza meta (e l'istanza non cambia)", () => {
    const tiny = { ...layout, spots: [], chatAreas: [] };
    const empty = new Map();
    assert.equal(tick(tiny, KEYS, empty, 0, seeded(1)), empty);
  });

  it("uno spot sparito dal layout fa perdere la meta", () => {
    const goals = tick(layout, ["a"], new Map(), 0, () => 0.99);
    const gone = goals.get("a").spotId;
    const reduced = { ...layout, spots: layout.spots.filter((s) => s.id !== gone) };
    const next = tick(reduced, ["a"], goals, 1, seeded(2));
    assert.notEqual(next.get("a").spotId, gone);
  });

  it("mai due bot sullo stesso spot, per molti tick e molte stanze", () => {
    for (const deskCount of [4, 8, 12]) {
      const room = roomLayout(deskCount);
      for (let seed = 0; seed < 25; seed++) {
        const random = seeded(seed * 31 + deskCount);
        const all = Array.from({ length: deskCount }, (_, i) => `k${i}`);
        let goals = new Map();
        let now = 0;
        for (let t = 0; t < 60; t++) {
          now += 1 + Math.floor(random() * 3000);
          // Ogni tanto qualche bot esce o rientra dall'idle.
          const idle = all.filter(() => random() < 0.8);
          goals = tick(room, idle, goals, now, random);
          checkPlan(room, idle, goals, now);
        }
      }
    }
  });

  it("con piu' bot che spot nessuno resta senza posto finche' ce ne sono", () => {
    const room = roomLayout(4);
    const capacity = room.spots.length;
    const many = Array.from({ length: capacity + 5 }, (_, i) => `k${i}`);
    const goals = tick(room, many, new Map(), 0, () => 0.99);
    assert.equal(goals.size, capacity);
    checkPlan(room, many, goals, 0);
  });
});

describe("restingGoals", () => {
  const layout = roomLayout(8);

  it("e' deterministico: prima il divano, poi il caffe', poi gli altri", () => {
    const idle = ["a", "b", "c", "d", "e", "f"];
    const goals = restingGoals(layout, idle);
    assert.deepEqual([...goals.keys()], idle);
    const kinds = [...goals.values()].map((g) => g.kind);
    assert.deepEqual(kinds.slice(0, 3), ["sofa", "sofa", "sofa"]);
    assert.deepEqual(kinds.slice(3, 5), ["coffee", "coffee"]);
    assert.notEqual(kinds[5], "sofa");
    assert.notEqual(kinds[5], "coffee");
    assert.deepEqual([...restingGoals(layout, idle)], [...goals]);
  });

  it("nessuna sovrapposizione e until infinito", () => {
    const idle = Array.from({ length: layout.spots.length }, (_, i) => `k${i}`);
    const goals = restingGoals(layout, idle);
    assert.equal(goals.size, idle.length);
    assert.equal(new Set([...goals.values()].map((g) => g.spotId)).size, idle.length);
    for (const g of goals.values()) assert.equal(g.until, Infinity);
  });

  it("troppi bot: i ritardatari restano senza posto; nessun bot: mappa vuota", () => {
    const idle = Array.from({ length: layout.spots.length + 2 }, (_, i) => `k${i}`);
    assert.equal(restingGoals(layout, idle).size, layout.spots.length);
    assert.equal(restingGoals(layout, []).size, 0);
  });
});

// ---------------------------------------------------------------- botPlacement

describe("botPlacement", () => {
  const layout = roomLayout(8);
  const desk = layout.desks[5];
  const place = (activity, goal, slot = 0) => botPlacement({ activity, desk: 5 }, layout, goal, slot);

  it("working: seduto alla scrivania, a digitare", () => {
    assert.deepEqual(place("working"), {
      x: desk.seat.x,
      y: desk.seat.y,
      facing: desk.facing,
      pose: "typing",
      speed: "walk",
    });
  });

  it("failed: seduto, accasciato", () => {
    const p = place("failed");
    assert.deepEqual([p.x, p.y, p.pose, p.speed], [desk.seat.x, desk.seat.y, "slumped", "walk"]);
  });

  it("attention: in piedi accanto alla scrivania, mano alzata, di corsa", () => {
    const p = place("attention");
    assert.deepEqual([p.x, p.y, p.pose, p.speed], [desk.standing.x, desk.standing.y, "raising-hand", "run"]);
    assert.equal(p.facing, desk.facing);
  });

  it("completed: in piedi accanto alla scrivania, festeggia", () => {
    const p = place("completed");
    assert.deepEqual([p.x, p.y, p.pose, p.speed], [desk.standing.x, desk.standing.y, "celebrating", "walk"]);
  });

  it("background: allo spot del server, poi (overflow) in piedi alla scrivania", () => {
    layout.server.forEach((s, i) => {
      const p = place("background", undefined, i);
      assert.deepEqual([p.x, p.y, p.facing, p.pose], [s.x, s.y, s.facing, "waiting"]);
    });
    for (const slot of [layout.server.length, layout.server.length + 7]) {
      const p = place("background", undefined, slot);
      assert.deepEqual([p.x, p.y, p.facing, p.pose], [desk.standing.x, desk.standing.y, desk.facing, "waiting"]);
    }
  });

  it("idle con meta: spot e posa in base al tipo", () => {
    const byKind = (kind) => layout.spots.find((s) => s.kind === kind);
    const expected = { coffee: "coffee", sofa: "sitting", window: "looking", plant: "looking", floor: "standing" };
    for (const [kind, pose] of Object.entries(expected)) {
      const s = byKind(kind);
      const p = place("idle", { spotId: s.id, kind, until: 1 });
      assert.deepEqual([p.x, p.y, p.facing, p.pose, p.speed], [s.x, s.y, s.facing, pose, "walk"], kind);
    }
  });

  it("idle in chiacchierata: posto dell'area, posa chatting", () => {
    const seat = layout.chatAreas[1].seats[2];
    const p = place("idle", { spotId: seat.id, kind: "chat", until: 1, chat: layout.chatAreas[1].id });
    assert.deepEqual([p.x, p.y, p.facing, p.pose], [seat.x, seat.y, seat.facing, "chatting"]);
  });

  it("idle senza meta o con spot sconosciuto: in piedi accanto alla scrivania", () => {
    for (const goal of [undefined, { spotId: "nope", kind: "sofa", until: 1 }]) {
      const p = place("idle", goal);
      assert.deepEqual(
        [p.x, p.y, p.facing, p.pose, p.speed],
        [desk.standing.x, desk.standing.y, desk.facing, "standing", "walk"],
      );
    }
  });

  it("la meta non conta se il bot non e' in idle", () => {
    const s = layout.spots[0];
    const p = place("working", { spotId: s.id, kind: s.kind, until: 1 });
    assert.equal(p.pose, "typing");
  });

  it("scrivania fuori dal layout: ripiega sulla prima", () => {
    const p = botPlacement({ activity: "working", desk: 99 }, layout, undefined, 0);
    assert.equal(p.x, layout.desks[0].seat.x);
  });
});
