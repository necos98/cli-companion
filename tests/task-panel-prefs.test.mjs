import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  TASK_PANEL_STORAGE_KEY,
  readCollapsed,
  withCollapsed,
  writeCollapsed,
} from "../src/lib/taskPanelPrefs.ts";

const memory = (initial) => {
  const data = new Map(initial === undefined ? [] : [[TASK_PANEL_STORAGE_KEY, initial]]);
  return {
    getItem: (k) => data.get(k) ?? null,
    setItem: (k, v) => void data.set(k, v),
    data,
  };
};

describe("pannello Task: stato compresso", () => {
  it("storage assente, vuoto o rotto = tutto espanso", () => {
    assert.deepEqual(readCollapsed(null), {});
    assert.deepEqual(readCollapsed(undefined), {});
    assert.deepEqual(readCollapsed(memory()), {});
    assert.deepEqual(readCollapsed(memory("{non json")), {});
    assert.deepEqual(readCollapsed(memory("[1,2]")), {});
    assert.deepEqual(readCollapsed(memory('"x"')), {});
  });
  it("legge solo i valori true", () => {
    assert.deepEqual(readCollapsed(memory('{"a":true,"b":false,"c":1}')), { a: true });
  });
  it("accessor che lancia: nessun errore", () => {
    const boom = {
      getItem() {
        throw new Error("blocked");
      },
      setItem() {
        throw new Error("blocked");
      },
    };
    assert.deepEqual(readCollapsed(boom), {});
    assert.doesNotThrow(() => writeCollapsed(boom, { a: true }));
    assert.doesNotThrow(() => writeCollapsed(null, { a: true }));
  });
  it("scrittura e rilettura", () => {
    const s = memory();
    writeCollapsed(s, { w1: true });
    assert.deepEqual(readCollapsed(s), { w1: true });
  });
  it("withCollapsed non muta e toglie gli espansi", () => {
    const base = { a: true };
    const next = withCollapsed(base, "b", true);
    assert.deepEqual(next, { a: true, b: true });
    assert.deepEqual(base, { a: true });
    assert.deepEqual(withCollapsed(next, "a", false), { b: true });
  });
});
