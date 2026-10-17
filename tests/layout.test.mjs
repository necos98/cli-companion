import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  EMPTY_LAYOUT,
  GLOBAL_ID,
  activePaneId,
  closeTabIn,
  computePaneLayouts,
  gridDims,
  gridRects,
  focusTab,
  groupIndicator,
  openTab,
  paneRects,
  pinTab,
  selectGlobal,
  togglePin,
  unpinTab,
  reorderIds,
  reorderTabs,
  selectGroup,
  setActivePane,
  toggleMaximize,
} from "../src/lib/layout.ts";

const close = (a, b) => Math.abs(a - b) < 1e-9;

describe("griglia automatica", () => {
  it("dimensioni: cols = ceil(sqrt(n)), rows = ceil(n/cols)", () => {
    const want = { 1: [1, 1], 2: [2, 1], 3: [2, 2], 4: [2, 2], 5: [3, 2], 6: [3, 2], 7: [3, 3], 9: [3, 3], 10: [4, 3] };
    for (const [n, [cols, rows]] of Object.entries(want)) {
      assert.deepEqual(gridDims(Number(n)), { cols, rows }, `n=${n}`);
    }
    assert.deepEqual(gridDims(0), { cols: 0, rows: 0 });
    assert.deepEqual(gridRects(0), []);
    assert.deepEqual(gridRects(-3), []);
  });

  it("1 pannello = tutta l'area; 2 = affiancati", () => {
    assert.deepEqual(gridRects(1), [{ left: 0, top: 0, width: 100, height: 100 }]);
    assert.deepEqual(gridRects(2), [
      { left: 0, top: 0, width: 50, height: 100 },
      { left: 50, top: 0, width: 50, height: 100 },
    ]);
  });

  it("3 pannelli: due sopra, uno a tutta larghezza sotto", () => {
    const r = gridRects(3);
    assert.deepEqual(r[0], { left: 0, top: 0, width: 50, height: 50 });
    assert.deepEqual(r[1], { left: 50, top: 0, width: 50, height: 50 });
    assert.deepEqual(r[2], { left: 0, top: 50, width: 100, height: 50 });
  });

  it("5 pannelli: 3 sopra, 2 sotto che riempiono la larghezza", () => {
    const r = gridRects(5);
    assert.ok(close(r[2].width, 100 / 3));
    assert.ok(close(r[3].width, 50) && close(r[3].left, 0));
    assert.ok(close(r[4].width, 50) && close(r[4].left, 50));
    assert.ok(close(r[4].top, 50) && close(r[4].height, 50));
  });

  it("copre sempre tutta l'area senza sovrapposizioni (1..16)", () => {
    for (let n = 1; n <= 16; n++) {
      const rects = gridRects(n);
      assert.equal(rects.length, n);
      const area = rects.reduce((a, r) => a + r.width * r.height, 0);
      assert.ok(close(area, 100 * 100), `n=${n} area=${area}`);
      for (const r of rects) {
        assert.ok(r.left >= -1e-9 && r.top >= -1e-9);
        assert.ok(r.left + r.width <= 100 + 1e-9 && r.top + r.height <= 100 + 1e-9);
      }
      for (let i = 0; i < n; i++) {
        for (let j = i + 1; j < n; j++) {
          const a = rects[i];
          const b = rects[j];
          const overlapX = Math.min(a.left + a.width, b.left + b.width) - Math.max(a.left, b.left);
          const overlapY = Math.min(a.top + a.height, b.top + b.height) - Math.max(a.top, b.top);
          assert.ok(!(overlapX > 1e-9 && overlapY > 1e-9), `n=${n} ${i}/${j} si sovrappongono`);
        }
      }
    }
  });
});

describe("riordino", () => {
  it("sposta sulla posizione del bersaglio, gli altri scorrono", () => {
    assert.deepEqual(reorderIds(["a", "b", "c", "d"], "a", "c"), ["b", "c", "a", "d"]);
    assert.deepEqual(reorderIds(["a", "b", "c", "d"], "d", "a"), ["d", "a", "b", "c"]);
    assert.deepEqual(reorderIds(["a", "b", "c"], "c", "b"), ["a", "c", "b"]);
  });

  it("id uguali o sconosciuti: stessa istanza, nessuna modifica", () => {
    const o = ["a", "b"];
    assert.equal(reorderIds(o, "a", "a"), o);
    assert.equal(reorderIds(o, "x", "a"), o);
    assert.equal(reorderIds(o, "a", "x"), o);
  });

  it("non muta l'input", () => {
    const o = ["a", "b", "c"];
    reorderIds(o, "a", "c");
    assert.deepEqual(o, ["a", "b", "c"]);
  });
});

describe("gruppi", () => {
  const open = (s, ws, id) => openTab(s, ws, id);

  it("aprire crea il gruppo in coda, lo attiva e rende attivo il pannello", () => {
    let s = open(EMPTY_LAYOUT, "w1", "a");
    s = open(s, "w2", "b");
    s = open(s, "w1", "c");
    assert.deepEqual(s.groups.map((g) => g.workspaceId), ["w1", "w2"]);
    assert.deepEqual(s.groups[0].order, ["a", "c"]);
    assert.equal(s.groups[0].activeTabId, "c");
    assert.equal(s.activeWorkspaceId, "w1");
  });

  it("aprire un pannello toglie il maximize del gruppo", () => {
    let s = open(EMPTY_LAYOUT, "w1", "a");
    s = toggleMaximize(s, "a");
    assert.equal(s.groups[0].maximizedTabId, "a");
    s = open(s, "w1", "b");
    assert.equal(s.groups[0].maximizedTabId, null);
  });

  it("chiudere l'ultimo pannello fa sparire il gruppo e seleziona un altro", () => {
    let s = open(EMPTY_LAYOUT, "w1", "a");
    s = open(s, "w2", "b");
    s = open(s, "w3", "c");
    s = selectGroup(s, "w2");
    s = closeTabIn(s, "b");
    assert.deepEqual(s.groups.map((g) => g.workspaceId), ["w1", "w3"]);
    assert.equal(s.activeWorkspaceId, "w3");
    s = closeTabIn(s, "c");
    assert.equal(s.activeWorkspaceId, "w1");
    s = closeTabIn(s, "a");
    assert.deepEqual(s, EMPTY_LAYOUT);
  });

  it("chiudere un gruppo non attivo non cambia quello attivo", () => {
    let s = open(EMPTY_LAYOUT, "w1", "a");
    s = open(s, "w2", "b");
    s = closeTabIn(s, "a");
    assert.equal(s.activeWorkspaceId, "w2");
  });

  it("chiudere il pannello attivo passa al vicino; maximize si azzera", () => {
    let s = open(EMPTY_LAYOUT, "w1", "a");
    s = open(s, "w1", "b");
    s = open(s, "w1", "c");
    s = setActivePane(s, "b");
    s = toggleMaximize(s, "b");
    s = closeTabIn(s, "b");
    assert.deepEqual(s.groups[0].order, ["a", "c"]);
    assert.equal(s.groups[0].activeTabId, "c");
    assert.equal(s.groups[0].maximizedTabId, null);
    s = closeTabIn(s, "zzz");
    assert.deepEqual(s.groups[0].order, ["a", "c"]);
  });

  it("selectGroup ignora workspace senza gruppo; setActivePane attiva il gruppo", () => {
    let s = open(EMPTY_LAYOUT, "w1", "a");
    s = open(s, "w2", "b");
    assert.equal(selectGroup(s, "nope"), s);
    s = setActivePane(s, "a");
    assert.equal(s.activeWorkspaceId, "w1");
    assert.equal(setActivePane(s, "a"), s);
  });

  it("reorderTabs solo nello stesso gruppo", () => {
    let s = open(EMPTY_LAYOUT, "w1", "a");
    s = open(s, "w1", "b");
    s = open(s, "w2", "x");
    assert.deepEqual(reorderTabs(s, "a", "b").groups[0].order, ["b", "a"]);
    assert.equal(reorderTabs(s, "a", "x"), s);
  });

  it("paneRects: maximize = un solo rettangolo a tutta area", () => {
    let s = open(EMPTY_LAYOUT, "w1", "a");
    s = open(s, "w1", "b");
    assert.deepEqual(Object.keys(paneRects(s.groups[0])), ["a", "b"]);
    s = toggleMaximize(s, "b");
    assert.deepEqual(paneRects(s.groups[0]), { b: { left: 0, top: 0, width: 100, height: 100 } });
    s = toggleMaximize(s, "b");
    assert.equal(s.groups[0].maximizedTabId, null);
  });

  it("computePaneLayouts: visibile/focus/maximize per pannello", () => {
    let s = open(EMPTY_LAYOUT, "w1", "a");
    s = open(s, "w1", "b");
    s = open(s, "w2", "c");
    let l = computePaneLayouts(s);
    assert.equal(l.a.visible, false);
    assert.equal(l.c.visible, true);
    assert.equal(l.c.focused, true);
    s = selectGroup(s, "w1");
    l = computePaneLayouts(s);
    assert.deepEqual([l.a.visible, l.b.visible, l.c.visible], [true, true, false]);
    assert.deepEqual([l.a.focused, l.b.focused], [false, true]);
    s = toggleMaximize(s, "a");
    l = computePaneLayouts(s);
    assert.deepEqual([l.a.visible, l.b.visible], [true, false]);
    assert.equal(l.b.focused, false);
    assert.equal(l.a.maximized, true);
    assert.equal(l.b.rect, null);
  });
});

describe("indicatore di gruppo", () => {
  const dot = (tone) => ({ icon: "dot", tone });
  it("priorita': alert > check > busy > none", () => {
    assert.equal(groupIndicator([dot("busy"), { icon: "check", tone: "done" }, { icon: "alert", tone: "attention" }]), "alert");
    assert.equal(groupIndicator([dot("busy"), { icon: "check", tone: "done" }]), "check");
    assert.equal(groupIndicator([dot("idle"), dot("busy")]), "busy");
    assert.equal(groupIndicator([dot("idle")]), "none");
    assert.equal(groupIndicator([]), "none");
  });
});

describe("focusTab (click su notifica)", () => {
  const base = () => {
    let s = openTab(EMPTY_LAYOUT, "w1", "a");
    s = openTab(s, "w1", "b");
    s = openTab(s, "w2", "c");
    return s; // gruppo attivo w2
  };
  it("seleziona workspace e pannello", () => {
    const s = focusTab(base(), "a");
    assert.equal(s.activeWorkspaceId, "w1");
    assert.equal(s.groups[0].activeTabId, "a");
  });
  it("toglie il maximize di un altro pannello", () => {
    let s = toggleMaximize(base(), "b");
    s = focusTab(s, "a");
    assert.equal(s.groups[0].maximizedTabId, null);
    assert.equal(s.groups[0].activeTabId, "a");
  });
  it("mantiene il maximize dello stesso pannello", () => {
    let s = toggleMaximize(base(), "b");
    s = focusTab(s, "b");
    assert.equal(s.groups[0].maximizedTabId, "b");
    assert.equal(s.activeWorkspaceId, "w1");
  });
  it("tab inesistente o gia' a fuoco: stessa istanza", () => {
    const s = base();
    assert.equal(focusTab(s, "zzz"), s);
    assert.equal(focusTab(s, "c"), s);
  });
  it("non tocca l'ordine ne' gli altri gruppi", () => {
    const s = focusTab(base(), "a");
    assert.deepEqual(s.groups[0].order, ["a", "b"]);
    assert.deepEqual(s.groups[1], base().groups[1]);
  });
});

describe("vista globale", () => {
  // w1: a, b ; w2: c, d. Fissati: c, a (in questo ordine).
  const base = () => {
    let s = openTab(EMPTY_LAYOUT, "w1", "a");
    s = openTab(s, "w1", "b");
    s = openTab(s, "w2", "c");
    s = openTab(s, "w2", "d");
    s = pinTab(s, "c");
    return pinTab(s, "a");
  };

  it("pin accoda; doppio pin o tab sconosciuto: stessa istanza", () => {
    const s = base();
    assert.deepEqual(s.global.order, ["c", "a"]);
    assert.equal(s.global.activeTabId, "c");
    assert.equal(pinTab(s, "a"), s);
    assert.equal(pinTab(s, "zzz"), s);
    assert.equal(s.groups[0].order.includes("a"), true); // resta anche nel suo gruppo
  });

  it("unpin toglie, sceglie il vicino e azzera il maximize; vuota = torna ai workspace", () => {
    let s = selectGlobal(base());
    assert.equal(s.activeView, "global");
    s = toggleMaximize(s, "c");
    s = unpinTab(s, "c");
    assert.deepEqual(s.global, { order: ["a"], activeTabId: "a", maximizedTabId: null });
    assert.equal(unpinTab(s, "c"), s);
    s = unpinTab(s, "a");
    assert.deepEqual(s.global.order, []);
    assert.equal(s.activeView, "workspace");
    assert.equal(togglePin(s, "b").global.order[0], "b");
    assert.deepEqual(togglePin(togglePin(s, "b"), "b").global.order, []);
  });

  it("selectGlobal solo con tab fissati", () => {
    assert.equal(selectGlobal(openTab(EMPTY_LAYOUT, "w1", "a")).activeView, "workspace");
    const s = selectGlobal(base());
    assert.equal(selectGlobal(s), s);
  });

  it("selectGroup torna ai workspace anche sul gruppo gia' attivo", () => {
    const s = selectGroup(selectGlobal(base()), "w2");
    assert.equal(s.activeView, "workspace");
    assert.equal(s.activeWorkspaceId, "w2");
  });

  it("chiudere un tab lo toglie dalla vista globale (anche se mostrata)", () => {
    let s = selectGlobal(base());
    s = closeTabIn(s, "c");
    assert.deepEqual(s.global.order, ["a"]);
    assert.equal(s.global.activeTabId, "a");
    s = closeTabIn(s, "a");
    assert.deepEqual(s.global.order, []);
    assert.equal(s.activeView, "workspace");
    assert.equal(closeTabIn(s, "zzz"), s);
  });

  it("chiudere tutti i tab riporta a EMPTY_LAYOUT", () => {
    let s = base();
    for (const id of ["a", "b", "c", "d"]) s = closeTabIn(s, id);
    assert.deepEqual(s, EMPTY_LAYOUT);
  });

  it("computePaneLayouts: un solo rettangolo per tab, dalla vista mostrata", () => {
    let s = base(); // vista workspace, gruppo attivo w2 (c, d)
    let l = computePaneLayouts(s);
    assert.deepEqual([l.c.visible, l.d.visible, l.a.visible], [true, true, false]);
    assert.deepEqual([l.a.pinned, l.b.pinned, l.c.pinned], [true, false, true]);
    assert.equal(l.c.groupWorkspaceId, "w2");

    s = selectGlobal(s);
    l = computePaneLayouts(s);
    assert.deepEqual([l.c.visible, l.a.visible, l.b.visible, l.d.visible], [true, true, false, false]);
    assert.equal(l.c.groupWorkspaceId, GLOBAL_ID);
    assert.deepEqual(l.c.rect, { left: 0, top: 0, width: 50, height: 100 });
    assert.deepEqual(l.a.rect, { left: 50, top: 0, width: 50, height: 100 });
    assert.equal(l.c.focused, true);
    assert.equal(l.a.focused, false);
    assert.equal(l.d.rect, null);
  });

  it("riordino, attivo e maximize agiscono sulla vista globale, non sui gruppi", () => {
    let s = selectGlobal(base());
    s = reorderTabs(s, "c", "a");
    assert.deepEqual(s.global.order, ["a", "c"]);
    assert.deepEqual(s.groups[1].order, ["c", "d"]);
    assert.equal(reorderTabs(s, "a", "b"), s); // b non fissato
    s = setActivePane(s, "c");
    assert.equal(s.global.activeTabId, "c");
    assert.equal(s.activeView, "global");
    assert.equal(setActivePane(s, "c"), s);
    s = toggleMaximize(s, "a");
    assert.equal(s.global.maximizedTabId, "a");
    assert.equal(s.groups[0].maximizedTabId, null);
    assert.equal(activePaneId(s), "a");
    const l = computePaneLayouts(s);
    assert.deepEqual([l.a.visible, l.c.visible], [true, false]);
    assert.equal(l.a.maximized, true);
    s = toggleMaximize(s, "a");
    assert.equal(s.global.maximizedTabId, null);
  });

  it("activePaneId segue la vista mostrata", () => {
    const s = base();
    assert.equal(activePaneId(s), "d");
    assert.equal(activePaneId(selectGlobal(s)), "c");
    assert.equal(activePaneId(EMPTY_LAYOUT), null);
  });

  it("focusTab va al gruppo del workspace anche se il tab e' fissato", () => {
    const s = focusTab(selectGlobal(base()), "a");
    assert.equal(s.activeView, "workspace");
    assert.equal(s.activeWorkspaceId, "w1");
    assert.equal(s.groups[0].activeTabId, "a");
    assert.deepEqual(s.global.order, ["c", "a"]);
  });

  it("aprire un tab o attivare un pannello di workspace lascia la vista globale", () => {
    assert.equal(openTab(selectGlobal(base()), "w1", "e").activeView, "workspace");
    assert.equal(setActivePane(selectGlobal(base()), "b").activeView, "workspace");
  });
});
