import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  compareVersions,
  isNewerVersion,
  normalizeVersion,
  pickInstallerAsset,
} from "../src/lib/updater.ts";

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const read = (p) => readFileSync(path.join(root, p), "utf8");

describe("updater: config repo GitHub", () => {
  it("updater.config.json esiste con owner/repo/assetPatterns documentati", () => {
    const raw = read("updater.config.json");
    const cfg = JSON.parse(raw);
    assert.ok(typeof cfg.owner === "string" && cfg.owner.length > 0, "owner mancante");
    assert.ok(typeof cfg.repo === "string" && cfg.repo.length > 0, "repo mancante");
    assert.ok(
      Array.isArray(cfg.assetPatterns) && cfg.assetPatterns.length > 0,
      "assetPatterns mancanti",
    );
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
  it("updater.rs espone check/download/install + rollback + sha256 + feed releases", () => {
    const rs = read("src-tauri/src/updater.rs");
    for (const id of [
      "pub fn check_blocking",
      "pub fn download_update_blocking",
      "pub fn install_update_blocking",
      "fn sha256_bytes",
      "remove_file",
      "releases/latest",
    ]) {
      assert.ok(rs.includes(id), `${id} non trovato in updater.rs`);
    }
  });

  it("lib.rs registra i comandi updater", () => {
    const lib = read("src-tauri/src/lib.rs");
    for (const c of [
      "updater::get_update_config",
      "updater::check_update",
      "updater::download_update",
      "updater::install_update",
    ]) {
      assert.ok(lib.includes(c), `${c} non registrato in lib.rs`);
    }
  });

  it("main.rs offre --check-update/--version/--help", () => {
    const main = read("src-tauri/src/main.rs");
    assert.match(main, /--check-update/);
    assert.match(main, /--version/);
    assert.match(main, /--help/);
  });

  it("tauri-args copre updater.rs e la GUI invoca i comandi", () => {
    assert.match(read("tests/tauri-args.test.mjs"), /updater\.rs/);
    const ts = read("src/lib/tauri.ts");
    for (const c of ["get_update_config", "check_update", "download_update", "install_update"]) {
      assert.ok(ts.includes(`"${c}"`), `invoke "${c}" mancante in tauri.ts`);
    }
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

describe("updater: scelta asset installer", () => {
  it("preferisce .msi a .exe secondo i pattern, case-insensitive", () => {
    const names = ["app-0.2.0.exe", "app-0.2.0.msi", "app.AppImage"];
    assert.equal(pickInstallerAsset(names, [".msi", ".exe"]), "app-0.2.0.msi");
    assert.equal(pickInstallerAsset(names, [".exe"]), "app-0.2.0.exe");
    assert.equal(pickInstallerAsset(["a.zip"], [".msi"]), null);
    assert.equal(pickInstallerAsset(names, ["MSI"]), "app-0.2.0.msi");
  });
});
