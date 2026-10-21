import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { dropOrder, dropPositionAt, moveItem } from "../src/lib/workspaceOrder.ts";

describe("moveItem", () => {
  it("sposta avanti e indietro", () => {
    assert.deepEqual(moveItem(["a", "b", "c", "d"], 0, 2), ["b", "c", "a", "d"]);
    assert.deepEqual(moveItem(["a", "b", "c", "d"], 3, 1), ["a", "d", "b", "c"]);
  });
  it("indici uguali o fuori range: stessa istanza", () => {
    const l = ["a", "b"];
    assert.equal(moveItem(l, 1, 1), l);
    assert.equal(moveItem(l, -1, 0), l);
    assert.equal(moveItem(l, 0, 5), l);
  });
});

describe("dropOrder", () => {
  const ids = ["a", "b", "c", "d"];
  it("prima / dopo il bersaglio", () => {
    assert.deepEqual(dropOrder(ids, "a", "c", "before"), ["b", "a", "c", "d"]);
    assert.deepEqual(dropOrder(ids, "a", "c", "after"), ["b", "c", "a", "d"]);
    assert.deepEqual(dropOrder(ids, "d", "a", "before"), ["d", "a", "b", "c"]);
    assert.deepEqual(dropOrder(ids, "b", "d", "after"), ["a", "c", "d", "b"]);
  });
  it("posizione invariata o id sconosciuti: stessa istanza", () => {
    assert.equal(dropOrder(ids, "b", "c", "before"), ids);
    assert.equal(dropOrder(ids, "b", "a", "after"), ids);
    assert.equal(dropOrder(ids, "b", "b", "after"), ids);
    assert.equal(dropOrder(ids, "x", "a", "before"), ids);
    assert.equal(dropOrder(ids, "a", "x", "before"), ids);
  });
});

describe("dropPositionAt", () => {
  it("metà superiore = prima, inferiore = dopo", () => {
    assert.equal(dropPositionAt(10, 0, 40), "before");
    assert.equal(dropPositionAt(30, 0, 40), "after");
  });
});
