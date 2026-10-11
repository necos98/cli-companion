import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { clipboardKeyAction, resolvePasteAction } from "../src/lib/terminal.ts";

const key = (over) => ({ key: "c", ctrlKey: false, shiftKey: false, ...over });

describe("terminale: tasti copia/incolla", () => {
  it("Ctrl+C con selezione = copia, senza = SIGINT (null)", () => {
    assert.equal(clipboardKeyAction(key({ ctrlKey: true }), true), "copy");
    assert.equal(clipboardKeyAction(key({ ctrlKey: true }), false), null);
  });

  it("Ctrl+Shift+C = copia, Ctrl+V e Ctrl+Shift+V = incolla", () => {
    assert.equal(
      clipboardKeyAction(key({ ctrlKey: true, shiftKey: true }), false),
      "copy",
    );
    assert.equal(
      clipboardKeyAction(key({ key: "v", ctrlKey: true }), false),
      "paste",
    );
    assert.equal(
      clipboardKeyAction(key({ key: "V", ctrlKey: true, shiftKey: true }), true),
      "paste",
    );
  });

  it("Ctrl+Ins = copia, Shift+Ins = incolla, Ins solo = null", () => {
    assert.equal(
      clipboardKeyAction(key({ key: "Insert", ctrlKey: true }), true),
      "copy",
    );
    assert.equal(
      clipboardKeyAction(key({ key: "Insert", shiftKey: true }), false),
      "paste",
    );
    assert.equal(clipboardKeyAction(key({ key: "Insert" }), false), null);
  });

  it("altri tasti = null", () => {
    assert.equal(clipboardKeyAction(key({}), false), null);
    assert.equal(clipboardKeyAction(key({ key: "c" }), true), null);
    assert.equal(
      clipboardKeyAction(key({ key: "x", ctrlKey: true }), true),
      null,
    );
  });
});

describe("terminale: risoluzione incolla", () => {
  it("testo non vuoto = incolla il testo una sola volta", () => {
    assert.deepEqual(resolvePasteAction("ciao"), {
      kind: "paste",
      text: "ciao",
    });
  });

  it("testo vuoto (es. immagine) = inoltra Ctrl+V grezzo al PTY", () => {
    assert.deepEqual(resolvePasteAction(""), {
      kind: "forward-key",
      key: "\x16",
    });
  });
});
