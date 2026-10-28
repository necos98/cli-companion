import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  configFromDraft,
  draftFromConfig,
  draftIsSaved,
  emptyDraft,
  repoRows,
  textToLines,
} from "../src/lib/repoConfig.ts";

describe("repoConfig: bozza <-> configurazione", () => {
  it("righe non vuote con trim", () => {
    assert.deepEqual(textToLines(" a \r\n\n b\n  "), ["a", "b"]);
  });
  it("converte e normalizza", () => {
    const d = {
      ...emptyDraft(),
      base_branch: " develop ",
      container_root: "/var/www/html/",
      copy_ignored: "configuration.php\n.\\public\\api\\.env\nconfiguration.php",
      setup: "composer dump-autoload\n",
      cleanup: "rm -rf x",
      remove_force: true,
    };
    const r = configFromDraft(d);
    assert.equal(r.error, null);
    assert.deepEqual(r.config, {
      base_branch: "develop",
      container_root: "/var/www/html",
      copy_ignored: ["configuration.php", "public/api/.env"],
      setup: ["composer dump-autoload"],
      cleanup: ["rm -rf x"],
      remove_force: true,
    });
    assert.deepEqual(draftFromConfig(r.config).copy_ignored, "configuration.php\npublic/api/.env");
    assert.equal(draftIsSaved(d, r.config), true);
    assert.equal(draftIsSaved({ ...d, setup: "x" }, r.config), false);
    assert.equal(draftIsSaved(emptyDraft(), undefined), true);
  });
  it("rifiuta input non valido", () => {
    for (const p of ["../x", "/etc/passwd", "C:\\x", "a/../../b", ".."]) {
      assert.notEqual(configFromDraft({ ...emptyDraft(), copy_ignored: p }).error, null, p);
    }
    for (const b of ["-x", "a b", "a..b", "x/"]) {
      assert.notEqual(configFromDraft({ ...emptyDraft(), base_branch: b }).error, null, b);
    }
    assert.notEqual(configFromDraft({ ...emptyDraft(), setup: "x".repeat(2001) }).error, null);
    assert.equal(configFromDraft({ ...emptyDraft(), setup: "x".repeat(2000) }).error, null);
    assert.notEqual(configFromDraft({ ...emptyDraft(), cleanup: Array(31).fill("x").join("\n") }).error, null);
  });
  it("E2E: bozza vuota = nessuna configurazione, altrimenti validata e normalizzata", () => {
    const none = configFromDraft(emptyDraft());
    assert.equal("e2e" in none.config, false);
    const d = { ...emptyDraft(), e2e_command: " npm run e2e ", e2e_junit: ".\\out\\junit.xml", e2e_timeout: "" };
    const r = configFromDraft(d);
    assert.equal(r.error, null);
    assert.deepEqual(r.config.e2e, { command: "npm run e2e", junit: "out/junit.xml", timeout_min: 20 });
    assert.equal(draftFromConfig(r.config).e2e_timeout, "20");
    assert.equal(draftIsSaved(d, r.config), true);
    assert.equal(draftIsSaved({ ...d, e2e_timeout: "5" }, r.config), false);
    assert.equal(draftIsSaved(emptyDraft(), { ...r.config, e2e: undefined }), true);
  });
  it("E2E: rifiuta input non valido", () => {
    const ok = { ...emptyDraft(), e2e_command: "x", e2e_junit: "j.xml" };
    assert.equal(configFromDraft(ok).error, null);
    const bad = [
      { e2e_command: "" },
      { e2e_command: "  ", e2e_timeout: "5" },
      { e2e_command: "a\nb" },
      { e2e_command: "x".repeat(2001) },
      { e2e_junit: "" },
      { e2e_junit: "../j.xml" },
      { e2e_junit: "/etc/j" },
      { e2e_junit: "C:\\j.xml" },
      { e2e_timeout: "0" },
      { e2e_timeout: "61" },
      { e2e_timeout: "1.5" },
      { e2e_timeout: "abc" },
    ];
    for (const over of bad) assert.notEqual(configFromDraft({ ...ok, ...over }).error, null, JSON.stringify(over));
    assert.equal(configFromDraft({ ...ok, e2e_command: "x".repeat(2000) }).error, null);
    for (const t of ["1", "60"]) assert.equal(configFromDraft({ ...ok, e2e_timeout: t }).error, null, t);
  });
  it("righe: rilevati più configurati non trovati", () => {
    assert.deepEqual(repoRows(["a", "b"], ["b", "c"]), [
      { repo: "a", missing: false },
      { repo: "b", missing: false },
      { repo: "c", missing: true },
    ]);
    // Rilevamento ancora in corso: nessuno e' "non trovato".
    assert.deepEqual(repoRows(null, ["c"]), [{ repo: "c", missing: false }]);
  });
});
