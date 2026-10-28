import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  BRANCH_DEFAULT_TEXT,
  EMPTY_TEXT,
  LAUNCH_INHERIT_TEXT,
  LAUNCH_SHELL_TEXT,
  LONG_TEXT_MIN,
  MODEL_DEFAULT_TEXT,
  NOTE_RUNS_COMMANDS,
  NOTE_WIDENS_PERMISSIONS,
  UNKNOWN_TEXT,
  diffLaunches,
  repoGroupTitle,
  sensitiveNote,
  settingsProposalRows,
} from "../src/lib/settingsProposal.ts";
import { pendingProposals, taskCounts } from "../src/lib/board.ts";

const rows = (patch, current = {}, sensitive = []) => settingsProposalRows({ patch, current, sensitive });
const byField = (list, field) => list.find((r) => r.field === field);

describe("settingsProposalRows: valori singoli", () => {
  it("testo, modello null ed enum con le etichette della UI", () => {
    const r = rows(
      { worker_rules: "Niente test", worker_model: null, worker_permission: "bypass" },
      { worker_rules: "", worker_model: "opus", worker_permission: "accept-edits" },
    );
    assert.deepEqual(
      r.map((x) => [x.field, x.label, x.before, x.after]),
      [
        ["worker_rules", "Regole per i worker", EMPTY_TEXT, "Niente test"],
        ["worker_model", "Modello dei worker", "opus", MODEL_DEFAULT_TEXT],
        ["worker_permission", "Permessi dei worker automatici", "Approva modifiche ai file", "Nessuna conferma (bypass)"],
      ],
    );
  });

  it("valore attuale non noto = null; permesso sconosciuto resta com'e'", () => {
    const [r] = rows({ worker_permission: "futuro" });
    assert.equal(r.before, null);
    assert.equal(r.after, "futuro");
    assert.equal(UNKNOWN_TEXT, "—");
  });

  it("i testi lunghi o su piu' righe sono comprimibili", () => {
    const [short] = rows({ worker_rules: "breve" }, { worker_rules: "x" });
    const [multi] = rows({ worker_rules: "a\nb" }, { worker_rules: "x" });
    const [long] = rows({ worker_rules: "y".repeat(LONG_TEXT_MIN + 1) }, { worker_rules: "x" });
    assert.deepEqual([short.long, multi.long, long.long], [false, true, true]);
    assert.equal(long.after.length, LONG_TEXT_MIN + 1);
  });

  it("consegna: solo i campi proposti, con le etichette dei form", () => {
    const r = rows(
      { delivery: { mode: "pr", merge_method: "rebase" }, use_schedule: true },
      { delivery: { mode: "commit", merge_method: "squash" }, use_schedule: false },
    );
    assert.deepEqual(
      r.map((x) => [x.field, x.before, x.after]),
      [
        ["delivery.mode", "Solo commit", "Branch + PR + merge"],
        ["delivery.merge_method", "Squash", "Rebase"],
        ["use_schedule", "Spento", "Acceso"],
      ],
    );
  });

  it("patch vuota = nessuna riga", () => {
    assert.deepEqual(rows({}), []);
  });
});

describe("settingsProposalRows: liste", () => {
  it("allow/deny diventano righe aggiunte e rimosse", () => {
    const r = rows(
      { worker_allow: ["Bash(git:*)", "Edit"], worker_deny: [] },
      { worker_allow: ["Edit", "Bash(rm:*)"], worker_deny: ["WebFetch"] },
    );
    const allow = byField(r, "worker_allow");
    assert.equal(allow.kind, "list");
    assert.deepEqual(allow.removed, ["Bash(rm:*)"]);
    assert.deepEqual(allow.added, ["Bash(git:*)"]);
    assert.deepEqual(byField(r, "worker_deny").removed, ["WebFetch"]);
    assert.deepEqual(byField(r, "worker_deny").added, []);
  });

  it("stessa lista in altro ordine = nessuna differenza", () => {
    const [r] = rows({ worker_allow: ["a", "b"] }, { worker_allow: ["b", "a"] });
    assert.deepEqual([r.removed, r.added], [[], []]);
  });
});

describe("settingsProposalRows: avvii", () => {
  const cur = [
    { id: "l1", name: "Dev", command: "npm run dev" },
    { id: "l2", name: "Test", command: "npm test" },
    { id: "l3", name: "Old", command: "old" },
  ];

  it("confronta per id e per nome", () => {
    const d = diffLaunches(cur, [
      { id: "l1", name: "Dev", command: "npm start" }, // cambiato (per id)
      { id: "l2", name: "Test", command: "npm test" }, // invariato
      { name: "Old", command: "old" }, // stesso nome senza id = invariato
      { name: "Nuovo", command: "echo" }, // nuovo
    ]);
    assert.deepEqual(d.removed, ["Dev: npm run dev"]);
    assert.deepEqual(d.added, ["Dev: npm start", "Nuovo: echo"]);
  });

  it("gli avvii attuali non piu' presenti sono rimossi", () => {
    const d = diffLaunches(cur, [{ id: "l1", name: "Dev", command: "npm run dev" }]);
    assert.deepEqual(d.removed, ["Test: npm test", "Old: old"]);
    assert.deepEqual(d.added, []);
  });

  it("rinomina per id", () => {
    const d = diffLaunches(cur, [{ id: "l3", name: "Nuovo nome", command: "old" }, cur[0], cur[1]]);
    assert.deepEqual(d.removed, ["Old: old"]);
    assert.deepEqual(d.added, ["Nuovo nome: old"]);
  });

  it("senza avvii attuali sono tutti nuovi; due omonimi non si confondono", () => {
    assert.deepEqual(diffLaunches(undefined, [{ name: "A", command: "a" }]).added, ["A: a"]);
    const d = diffLaunches([{ id: "x", name: "A", command: "1" }], [
      { name: "A", command: "1" },
      { name: "A", command: "2" },
    ]);
    assert.deepEqual(d.added, ["A: 2"]);
    assert.deepEqual(d.removed, []);
  });

  it("avvio predefinito: nome dal patch, shell e null", () => {
    const patch = { launches: [{ id: "n1", name: "Nuovo", command: "x" }], default_launch: "n1" };
    const r = rows(patch, { launches: [], default_launch: null });
    const d = byField(r, "default_launch");
    assert.equal(d.before, LAUNCH_INHERIT_TEXT);
    assert.equal(d.after, "Nuovo");
    assert.equal(rows({ default_launch: "shell" }, { default_launch: "l1" })[0].after, LAUNCH_SHELL_TEXT);
    assert.equal(rows({ default_launch: null }, { default_launch: "builtin" })[0].after, LAUNCH_INHERIT_TEXT);
    assert.equal(rows({ default_launch: null }, { default_launch: "builtin" })[0].before, "builtin");
  });
});

describe("settingsProposalRows: repository", () => {
  it("un gruppo per chiave, \".\" = root, liste e valori", () => {
    const r = rows(
      {
        repos: {
          ".": { base_branch: "develop", setup: ["npm i", "npm run build"], remove_force: true },
          "api/web": { copy_ignored: [".env"], container_root: null },
        },
      },
      {
        repos: {
          ".": { base_branch: null, setup: ["npm i"], remove_force: false },
          "api/web": { copy_ignored: [], container_root: "/srv" },
        },
      },
    );
    assert.deepEqual(
      r.map((x) => [x.group, x.field]),
      [
        [repoGroupTitle("."), "repos[.].base_branch"],
        [repoGroupTitle("."), "repos[.].setup"],
        [repoGroupTitle("."), "repos[.].remove_force"],
        [repoGroupTitle("api/web"), "repos[api/web].container_root"],
        [repoGroupTitle("api/web"), "repos[api/web].copy_ignored"],
      ],
    );
    assert.equal(repoGroupTitle("."), "Repository: root del workspace");
    assert.equal(byField(r, "repos[.].base_branch").before, BRANCH_DEFAULT_TEXT);
    assert.equal(byField(r, "repos[.].base_branch").after, "develop");
    assert.deepEqual(byField(r, "repos[.].setup").added, ["npm run build"]);
    assert.equal(byField(r, "repos[.].remove_force").after, "Acceso");
    assert.equal(byField(r, "repos[api/web].container_root").after, "Nessuno");
  });

  it("valore null = rimuovi configurazione", () => {
    const [r] = rows({ repos: { gone: null } }, { repos: { gone: { setup: ["x"] } } });
    assert.deepEqual(r, {
      kind: "remove",
      field: "repos[gone]",
      group: repoGroupTitle("gone"),
      label: "Rimuovi configurazione",
      sensitive: null,
    });
  });

  it("repository senza configurazione salvata: valori di default, tutto aggiunto", () => {
    const r = rows({ repos: { new: { setup: ["a"], remove_force: false } } }, { repos: { new: null } });
    assert.deepEqual(byField(r, "repos[new].setup").added, ["a"]);
    assert.deepEqual(byField(r, "repos[new].setup").removed, []);
    assert.equal(byField(r, "repos[new].remove_force").before, "Spento");
  });

  it("workspace non noto (current vuoto): prima non noto", () => {
    const r = rows({ repos: { a: { base_branch: "x", remove_force: true } } });
    assert.equal(byField(r, "repos[a].base_branch").before, null);
    assert.equal(byField(r, "repos[a].remove_force").before, null);
  });
});

describe("settingsProposalRows: campi sensibili", () => {
  const patch = {
    worker_permission: "bypass",
    worker_allow: ["Bash"],
    worker_rules: "r",
    launches: [{ name: "A", command: "x" }],
    repos: { ".": { setup: ["s"], cleanup: ["c"], base_branch: "b" }, gone: null },
  };
  const sensitive = [
    "worker_permission",
    "worker_allow",
    "launches",
    "repos[.].setup",
    "repos[.].cleanup",
  ];

  it("evidenzia solo i campi elencati dal backend, con la nota giusta", () => {
    const r = rows(patch, {}, sensitive);
    const notes = Object.fromEntries(r.map((x) => [x.field, x.sensitive]));
    assert.deepEqual(notes, {
      worker_rules: null,
      worker_permission: NOTE_WIDENS_PERMISSIONS,
      worker_allow: NOTE_WIDENS_PERMISSIONS,
      launches: NOTE_RUNS_COMMANDS,
      "repos[.].base_branch": null,
      "repos[.].setup": NOTE_RUNS_COMMANDS,
      "repos[.].cleanup": NOTE_RUNS_COMMANDS,
      "repos[gone]": null,
    });
  });

  it("sensitiveNote", () => {
    assert.equal(sensitiveNote("worker_deny", ["worker_deny"]), NOTE_WIDENS_PERMISSIONS);
    assert.equal(sensitiveNote("default_launch", ["default_launch"]), NOTE_RUNS_COMMANDS);
    assert.equal(sensitiveNote("worker_rules", ["worker_permission"]), null);
  });
});

describe("conteggio delle proposte", () => {
  it("include quelle di impostazioni", () => {
    const board = { tasks: [], proposals: [{}], settings_proposals: [{}, {}] };
    assert.equal(pendingProposals(board), 3);
    assert.equal(taskCounts(board).proposals, 3);
    assert.equal(pendingProposals({ proposals: [{}] }), 1);
    assert.equal(pendingProposals(null), 0);
  });
});
