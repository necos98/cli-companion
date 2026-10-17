import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  EMPTY_LAYOUT,
  closeTabIn,
  computePaneLayouts,
  gridDims,
  gridRects,
  groupIndicator,
  openTab,
  paneRects,
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
