import assert from "node:assert/strict";
import { describe, it } from "node:test";

import {
  REVIEW_NEEDS_DEPENDENCY,
  isReview,
  proposalHeading,
  reviewDependencyError,
  reviewFindings,
  verdictBadge,
} from "../src/lib/review.ts";
import { buildPatch, formFromTask, validateTaskForm } from "../src/lib/taskForm.ts";

const review = (over = {}) => ({
  id: "r",
  title: "Verifica",
  task: "spec",
  kind: "review",
  status: "done",
  depends_on: ["a"],
  report: { status: "completed", summary: "ok", verdict: "issues", findings: ["bug 1", "  ", "bug 2"] },
  ...over,
});

describe("isReview", () => {
  it("riconosce solo kind review (assente = work)", () => {
    assert.equal(isReview({ kind: "review" }), true);
    assert.equal(isReview({ kind: "work" }), false);
    assert.equal(isReview({}), false);
  });
});

describe("verdictBadge", () => {
  it("Problemi trovati con colore di avviso, non di errore", () => {
    const b = verdictBadge(review());
    assert.equal(b.label, "Problemi trovati");
    assert.match(b.className, /attention/);
    assert.doesNotMatch(b.className, /destructive/);
  });
  it("Verificato per passed", () => {
    const t = review({ report: { status: "completed", summary: "", verdict: "passed" } });
    assert.equal(verdictBadge(t).label, "Verificato");
  });
  it("niente per work, review non done o senza verdetto", () => {
    assert.equal(verdictBadge(review({ kind: "work" })), null);
    assert.equal(verdictBadge(review({ status: "running" })), null);
    assert.equal(verdictBadge(review({ report: null })), null);
    assert.equal(verdictBadge(review({ report: { status: "completed", summary: "" } })), null);
  });
});

describe("reviewFindings", () => {
  const withReport = (verdict, findings) =>
    review({ report: { status: "completed", summary: "", verdict, findings } });
  it("issues: Problemi trovati con tono di avviso e findings non vuoti", () => {
    assert.deepEqual(reviewFindings(review()), {
      title: "Problemi trovati",
      tone: "attention",
      items: ["bug 1", "bug 2"],
    });
  });
  it("passed: Suggerimenti con tono neutro", () => {
    assert.deepEqual(reviewFindings(withReport("passed", ["x", " "])), {
      title: "Suggerimenti",
      tone: "neutral",
      items: ["x"],
    });
  });
  it("niente senza findings, per i work o per una review non done", () => {
    assert.equal(reviewFindings(withReport("passed", [])), null);
    assert.equal(reviewFindings(withReport("passed", [" "])), null);
    assert.equal(reviewFindings(withReport("passed")), null);
    assert.equal(reviewFindings(withReport("issues", [])), null);
    assert.equal(reviewFindings(review({ kind: "work" })), null);
    assert.equal(reviewFindings(review({ status: "running" })), null);
    assert.equal(reviewFindings(review({ report: null })), null);
  });
});

describe("proposalHeading", () => {
  const tasks = [{ id: "review-x", title: "Verifica login" }];
  it("Task Intake senza source_task", () => {
    assert.deepEqual(proposalHeading({}, tasks), { title: "Proposta da Task Intake", origin: null });
    assert.deepEqual(proposalHeading({ source_task: null }, tasks).origin, null);
  });
  it("dal reviewer col titolo del task, altrimenti l'id", () => {
    assert.deepEqual(proposalHeading({ source_task: "review-x" }, tasks), {
      title: "Proposta dal reviewer",
      origin: "Verifica login",
    });
    assert.equal(proposalHeading({ source_task: "gone" }, tasks).origin, "gone");
  });
});

describe("form di review", () => {
  const base = { title: "t", task: "s", model: "", depends_on: [], kind: "work", e2e: false };
  it("reviewDependencyError solo per review senza dipendenze", () => {
    assert.equal(reviewDependencyError("review", []), REVIEW_NEEDS_DEPENDENCY);
    assert.equal(reviewDependencyError("review", ["a"]), undefined);
    assert.equal(reviewDependencyError("work", []), undefined);
    assert.equal(reviewDependencyError(undefined, []), undefined);
  });
  it("validateTaskForm blocca una review senza dipendenze", () => {
    assert.equal(validateTaskForm({ ...base, kind: "review" }).depends_on, REVIEW_NEEDS_DEPENDENCY);
    assert.deepEqual(validateTaskForm({ ...base, kind: "review", depends_on: ["a"] }), {});
    assert.deepEqual(validateTaskForm(base), {});
  });
  it("formFromTask e buildPatch gestiscono kind", () => {
    const t = { id: "x", title: "t", task: "s", depends_on: ["a"], attempts: 0, status: "queued" };
    const form = formFromTask(t);
    assert.equal(form.kind, "work");
    assert.deepEqual(buildPatch(t, form, false), {});
    assert.deepEqual(buildPatch(t, { ...form, kind: "review" }, false), { kind: "review" });
    const r = { ...t, kind: "review" };
    assert.equal(formFromTask(r).kind, "review");
    assert.deepEqual(buildPatch(r, { ...formFromTask(r), kind: "work" }, false), { kind: "work" });
  });
});

describe("flag E2E nel form", () => {
  const task = (over = {}) => ({ id: "a", title: "t", task: "s", depends_on: [], status: "queued", ...over });

  it("formFromTask legge il flag e buildPatch lo invia solo se cambia", () => {
    const t = task();
    const form = formFromTask(t);
    assert.equal(form.e2e, false);
    assert.deepEqual(buildPatch(t, form, false), {});
    assert.deepEqual(buildPatch(t, { ...form, e2e: true }, false), { e2e: true });
    const marked = task({ e2e: true });
    assert.equal(formFromTask(marked).e2e, true);
    assert.deepEqual(buildPatch(marked, { ...formFromTask(marked), e2e: false }, false), { e2e: false });
  });
});
