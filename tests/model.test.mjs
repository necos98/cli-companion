import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { MODEL_MAX, modelError, sanitizeModel } from "../src/lib/model.ts";

describe("model: nome del modello", () => {
  it("trim, vuoto = null", () => {
    assert.equal(sanitizeModel("  opus "), "opus");
    assert.equal(sanitizeModel("   "), null);
    assert.equal(sanitizeModel("x".repeat(100)).length, MODEL_MAX);
  });
  it("niente spazi interni ne' caratteri strani, massimo 64", () => {
    assert.ok(modelError("a b"));
    assert.equal(modelError(" opus "), null);
    assert.equal(modelError("claude-opus-4.5_x"), null);
    assert.equal(modelError(""), null);
    assert.ok(modelError("a/b"));
    assert.ok(modelError("modèl"));
    assert.ok(modelError("x".repeat(65)));
    assert.equal(modelError("x".repeat(64)), null);
  });
});
