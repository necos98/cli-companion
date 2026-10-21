import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  compareVersions,
  isNewerVersion,
  normalizeVersion,
} from "../src/lib/updater.ts";

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const read = (p) => readFileSync(path.join(root, p), "utf8");

describe("updater: config repo GitHub (CLI --check-update)", () => {
  it("updater.config.json esiste con owner/repo documentati", () => {
    const raw = read("updater.config.json");
    const cfg = JSON.parse(raw);
    assert.ok(typeof cfg.owner === "string" && cfg.owner.length > 0, "owner mancante");
    assert.ok(typeof cfg.repo === "string" && cfg.repo.length > 0, "repo mancante");
    assert.match(raw, /REPLACE_ME/, "placeholder repo-non-ancora-esistente non documentato");
  });

  it("backend gestisce repo non configurato con messaggio chiaro", () => {
    const rs = read("src-tauri/src/updater.rs");
    assert.match(rs, /PLACEHOLDER_OWNER/);
    assert.match(rs, /non ancora configurato/);
  });
});

describe("updater: versioni sincronizzate", () => {
  it("package.json, Cargo.toml e tauri.conf.json hanno la stessa versione", () => {
    const pkg = JSON.parse(read("package.json")).version;
    const cargo = read("src-tauri/Cargo.toml").match(/^version\s*=\s*"([^"]+)"/m)[1];
    const conf = JSON.parse(read("src-tauri/tauri.conf.json")).version;
    assert.equal(cargo, pkg, `Cargo.toml (${cargo}) != package.json (${pkg})`);
    assert.equal(conf, pkg, `tauri.conf.json (${conf}) != package.json (${pkg})`);
  });
});

describe("updater: modulo backend e integrazione", () => {
  it("updater.rs tiene solo il check leggero per la CLI", () => {
    const rs = read("src-tauri/src/updater.rs");
    assert.ok(rs.includes("pub fn check_blocking"), "check_blocking non trovato");
    assert.ok(rs.includes("releases/latest"), "feed releases non trovato");
    for (const gone of ["download_update_blocking", "install_update_blocking", "fn sha256_bytes"]) {
      assert.ok(!rs.includes(gone), `${gone} non dovrebbe piu esistere in updater.rs`);
    }
  });

  it("lib.rs registra i plugin updater/process e non i vecchi comandi", () => {
    const lib = read("src-tauri/src/lib.rs");
    assert.ok(lib.includes("tauri_plugin_updater::Builder::new().build()"));
    assert.ok(lib.includes("tauri_plugin_process::init()"));
    assert.ok(!lib.includes("updater::check_update"), "vecchio comando check_update ancora registrato");
  });

  it("tauri.conf.json: bundle NSIS firmato e endpoint latest.json", () => {
    const conf = JSON.parse(read("src-tauri/tauri.conf.json"));
    assert.deepEqual(conf.bundle.targets, ["nsis"]);
    assert.equal(conf.bundle.createUpdaterArtifacts, true);
    const up = conf.plugins.updater;
    assert.ok(typeof up.pubkey === "string" && up.pubkey.length > 0, "pubkey mancante");
    assert.equal(up.windows.installMode, "passive");
    assert.ok(
      up.endpoints.every((e) => e.startsWith("https://") && e.endsWith("/latest.json")),
      "endpoint latest.json non valido",
    );
  });

  it("capability default abilita updater e riavvio", () => {
    const cap = JSON.parse(read("src-tauri/capabilities/default.json"));
    assert.ok(cap.permissions.includes("updater:default"));
    assert.ok(cap.permissions.includes("process:allow-restart"));
  });

  it("main.rs offre --check-update/--version/--help", () => {
    const main = read("src-tauri/src/main.rs");
    assert.match(main, /--check-update/);
    assert.match(main, /--version/);
    assert.match(main, /--help/);
  });

  it("la GUI usa il plugin updater nelle impostazioni", () => {
    const ui = read("src/components/UpdateCheck.tsx");
    assert.match(ui, /plugin-updater/);
    assert.match(ui, /downloadAndInstall/);
    assert.match(ui, /relaunch/);
    assert.match(read("src/components/SettingsDialog.tsx"), /UpdateCheck/);
  });
});

describe("updater: confronto versioni (parità TS/Rust)", () => {
  it("normalizeVersion", () => {
    assert.equal(normalizeVersion("  v1.2.3 "), "1.2.3");
    assert.equal(normalizeVersion("V0.1.0"), "0.1.0");
    assert.equal(normalizeVersion(""), "0");
  });

  // Stessi vettori del test Rust compare_versioni_vettori in updater.rs.
  const cases = [
    ["0.1.0", "0.1.0", 0],
    ["0.1.0", "0.2.0", -1],
    ["v0.1.0", "0.1.1", -1],
    ["1.10.0", "1.9.0", 1],
    ["1.2", "1.2.0", 0],
    ["2.0.0", "10.0.0", -1],
    ["1.0.0-beta", "1.0.0", -1],
    ["1.0.0", "1.0.0-beta", 1],
  ];
  for (const [a, b, want] of cases) {
    it(`${a} vs ${b} => ${want}`, () => assert.equal(compareVersions(a, b), want));
  }

  it("isNewerVersion", () => {
    assert.equal(isNewerVersion("0.1.0", "0.2.0"), true);
    assert.equal(isNewerVersion("0.2.0", "0.1.0"), false);
    assert.equal(isNewerVersion("0.1.0", "0.1.0"), false);
  });
});
