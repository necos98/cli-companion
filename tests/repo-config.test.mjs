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
