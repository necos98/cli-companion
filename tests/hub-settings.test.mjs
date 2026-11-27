import { describe, it } from "node:test";
import assert from "node:assert/strict";
import {
  API_KEY_MAX,
  BASE_URL_MAX,
  EMPTY_DRAFT,
  HUB_UNAVAILABLE_NOTICE,
  MODEL_MAX,
  SECRETARY_TOOLS,
  SYSTEM_PROMPT_MAX,
  checkApiKey,
  checkDraft,
  checkProvider,
  checkSecretaryName,
  checkSession,
  draftOf,
  hubAvailable,
  isLocalEndpoint,
  modelSummary,
  secretariesSummary,
  sessionSummary,
  toggleChannel,
  toggleTool,
} from "../src/lib/hubSettings.ts";

const secretary = (name, extra = {}) => ({
  name,
  image: null,
  system_prompt: "You help.",
  tools: [],
  channels: { mode: "all" },
  ...extra,
});

describe("riepiloghi degli accordion", () => {
  it("isLocalEndpoint: localhost e loopback sono locali, il resto remoto", () => {
    assert.equal(isLocalEndpoint("http://127.0.0.1:8080/v1"), true);
    assert.equal(isLocalEndpoint("http://localhost:11434/v1"), true);
    assert.equal(isLocalEndpoint("http://[::1]:8080"), true);
    assert.equal(isLocalEndpoint("http://app.localhost/v1"), true);
    assert.equal(isLocalEndpoint("https://api.deepseek.com/v1"), false);
    assert.equal(isLocalEndpoint("http://127.0.0.1.evil.example/v1"), false);
    assert.equal(isLocalEndpoint("http://192.168.1.10:8080"), false);
  });

  it("isLocalEndpoint: vuoto o illeggibile = nessuna risposta", () => {
    assert.equal(isLocalEndpoint(""), null);
    assert.equal(isLocalEndpoint("   "), null);
    assert.equal(isLocalEndpoint("non un indirizzo"), null);
  });

  it("modelSummary: endpoint e stato della API key", () => {
    const view = (base_url, model, api_key_set) => ({ provider: { base_url, model }, api_key_set });
    assert.equal(modelSummary(view("http://127.0.0.1:8080/v1", "m", true)), "Endpoint locale · API key impostata");
    assert.equal(modelSummary(view("https://api.example.com/v1", "m", false)), "Endpoint remoto · nessuna API key");
    assert.equal(modelSummary(view("", "", false)), "Non configurato");
    assert.equal(modelSummary(view("", "", true)), "Non configurato");
    assert.equal(modelSummary(view("boh", "m", false)), "Endpoint non valido · nessuna API key");
  });

  it("sessionSummary", () => {
    assert.equal(sessionSummary({ idle_minutes: 20, max_steps: 8 }), "20 min · 8 passi");
    assert.equal(sessionSummary({ idle_minutes: 1, max_steps: 1 }), "1 min · 1 passo");
  });

  it("secretariesSummary: quanti, quanti configurati, lingua", () => {
    const list = [secretary("a"), secretary("b"), secretary("c", { system_prompt: "" })];
    assert.equal(secretariesSummary({ secretaries: list, language: "it" }), "3 segretari · 2 configurati · italiano");
    assert.equal(
      secretariesSummary({ secretaries: [list[0]], language: "en" }),
      "1 segretario · 1 configurato · english",
    );
    assert.equal(secretariesSummary({ secretaries: [], language: "fr" }), "Nessun segretario · français");
  });
});

describe("modello", () => {
  it("checkProvider: toglie spazi e barre finali", () => {
    assert.deepEqual(checkProvider("  http://127.0.0.1:8080/v1/ ", " model "), {
      ok: true,
      value: { baseUrl: "http://127.0.0.1:8080/v1", model: "model" },
    });
  });

  it("checkProvider: vuoti = non configurato", () => {
    assert.deepEqual(checkProvider("", ""), { ok: true, value: { baseUrl: "", model: "" } });
  });

  it("checkProvider: schema non http(s) e lunghezze oltre i limiti", () => {
    assert.equal(checkProvider("ftp://host", "m").ok, false);
    assert.equal(checkProvider("host:8080", "m").ok, false);
    assert.equal(checkProvider(`http://${"a".repeat(BASE_URL_MAX)}`, "m").ok, false);
    assert.equal(checkProvider("http://h", "m".repeat(MODEL_MAX + 1)).ok, false);
    assert.equal(checkProvider("http://h", "m".repeat(MODEL_MAX)).ok, true);
  });

  it("checkApiKey: non vuota e nel limite, senza spazi ai bordi", () => {
    assert.deepEqual(checkApiKey("  sk-123 \n"), { ok: true, value: "sk-123" });
    assert.equal(checkApiKey("").ok, false);
    assert.equal(checkApiKey("   ").ok, false);
    assert.equal(checkApiKey("k".repeat(API_KEY_MAX + 1)).ok, false);
    assert.equal(checkApiKey("k".repeat(API_KEY_MAX)).ok, true);
  });

  it("checkApiKey: l'errore non riporta la chiave", () => {
    const secret = "sk-segreta-".repeat(200);
    const result = checkApiKey(secret);
    assert.equal(result.ok, false);
    assert.ok(!result.reason.includes("sk-segreta"));
  });
});

describe("sessioni", () => {
  it("checkSession: numeri interi nei limiti del backend", () => {
    assert.deepEqual(checkSession("20", "8"), { ok: true, value: { idleMinutes: 20, maxSteps: 8 } });
    assert.deepEqual(checkSession(" 1 ", "20"), { ok: true, value: { idleMinutes: 1, maxSteps: 20 } });
    assert.deepEqual(checkSession("240", "1"), { ok: true, value: { idleMinutes: 240, maxSteps: 1 } });
  });

  it("checkSession: fuori limite, decimali, vuoti o non numeri", () => {
    for (const [idle, steps] of [
      ["0", "8"],
      ["241", "8"],
      ["20", "0"],
      ["20", "21"],
      ["2.5", "8"],
      ["20", "1e1"],
      ["", "8"],
      ["20", ""],
      ["venti", "8"],
      ["-3", "8"],
    ]) {
      assert.equal(checkSession(idle, steps).ok, false, `${idle} / ${steps}`);
    }
  });
});

describe("segretari", () => {
  it("checkSecretaryName: normalizza e accetta lettere, cifre e trattini", () => {
    assert.deepEqual(checkSecretaryName("  Mario ", []), { ok: true, value: "mario" });
    assert.deepEqual(checkSecretaryName("bot-2", []), { ok: true, value: "bot-2" });
    assert.deepEqual(checkSecretaryName("a".repeat(24), []).ok, true);
  });

  it("checkSecretaryName: vuoto, lungo, caratteri non ammessi, trattini ai bordi", () => {
    for (const bad of ["", "   ", "a".repeat(25), "mario rossi", "mário", "mario_1", "-mario", "mario-", "@mario"]) {
      assert.equal(checkSecretaryName(bad, []).ok, false, bad);
    }
  });

  it("checkSecretaryName: rifiuta il nome di un altro segretario, senza badare alle maiuscole", () => {
    assert.equal(checkSecretaryName("Mario", ["mario"]).ok, false);
    assert.equal(checkSecretaryName("luisa", ["mario"]).ok, true);
  });

  it("draftOf e bozza vuota", () => {
    assert.deepEqual(EMPTY_DRAFT, { name: "", systemPrompt: "", tools: [], channels: null });
    assert.deepEqual(
      draftOf(secretary("a", { tools: ["thread_read"], channels: { mode: "only", channels: ["ws1"] } })),
      {
        name: "a",
        systemPrompt: "You help.",
        tools: ["thread_read"],
        channels: ["ws1"],
      },
    );
    assert.equal(draftOf(secretary("a")).channels, null);
  });

  it("toggleTool: aggiunge e toglie, sempre nell'ordine canonico", () => {
    const order = SECRETARY_TOOLS.map((t) => t.id);
    assert.deepEqual(toggleTool([], order[2]), [order[2]]);
    assert.deepEqual(toggleTool([order[2]], order[0]), [order[0], order[2]]);
    assert.deepEqual(toggleTool([order[0], order[2]], order[0]), [order[2]]);
  });

  it("SECRETARY_TOOLS: tutti i tool del backend, nell'ordine canonico", () => {
    assert.deepEqual(
      SECRETARY_TOOLS.map((t) => t.id),
      [
        "board_status",
        "board_overview",
        "task_details",
        "thread_read",
        "post_message",
        "open_plan_task",
        "decide_proposal",
        "relay_answer",
        "task_action",
        "board_control",
        "request_task_action",
        "request_task_update",
        "set_thread_state",
      ],
    );
    assert.ok(SECRETARY_TOOLS.every((t) => t.description.length > 0));
    const text = Object.fromEntries(SECRETARY_TOOLS.map((t) => [t.id, t.description]));
    assert.equal(text.open_plan_task, "Apre un task di pianificazione");
    assert.equal(text.decide_proposal, "Accetta o scarta la proposta a cui rispondi");
    assert.equal(text.relay_answer, "Risponde a un worker al posto tuo, quando glielo chiedi");
    assert.equal(text.task_action, "Ritenta un task fallito o ne cambia la posizione nella coda");
    assert.equal(text.board_control, "Mette in pausa o riprende la board e ne regola i task in parallelo");
    assert.equal(
      text.request_task_action,
      "Ti chiede di fermare, eliminare o chiudere un task: lo esegui tu con un clic",
    );
    assert.equal(
      text.request_task_update,
      "Ti chiede di cambiare titolo, specifica, dipendenze o modello di un task: lo confermi tu con un clic",
    );
    assert.equal(text.set_thread_state, "Chiude o riapre questo thread, quando glielo chiedi");
    assert.deepEqual(EMPTY_DRAFT.tools, []);
  });

  it("toggleChannel: nell'ordine dei canali esistenti", () => {
    const existing = ["general", "ws1", "ws2"];
    assert.deepEqual(toggleChannel([], "ws2", existing), ["ws2"]);
    assert.deepEqual(toggleChannel(["ws2"], "general", existing), ["general", "ws2"]);
    assert.deepEqual(toggleChannel(["general", "ws2"], "ws2", existing), ["general"]);
  });

  it("checkDraft: normalizza nome e prompt", () => {
    const result = checkDraft({ ...EMPTY_DRAFT, name: " Mario ", systemPrompt: "  You help.\n" }, []);
    assert.deepEqual(result, {
      ok: true,
      value: { name: "mario", systemPrompt: "You help.", tools: [], channels: null },
    });
  });

  it("checkDraft: prompt vuoto ammesso (non configurato)", () => {
    assert.equal(checkDraft({ ...EMPTY_DRAFT, name: "mario" }, []).ok, true);
  });

  it("checkDraft: nome non valido, prompt troppo lungo, nessun canale scelto", () => {
    assert.equal(checkDraft({ ...EMPTY_DRAFT, name: "" }, []).ok, false);
    assert.equal(checkDraft({ ...EMPTY_DRAFT, name: "mario" }, ["mario"]).ok, false);
    assert.equal(
      checkDraft({ ...EMPTY_DRAFT, name: "mario", systemPrompt: "x".repeat(SYSTEM_PROMPT_MAX + 1) }, []).ok,
      false,
    );
    assert.equal(checkDraft({ ...EMPTY_DRAFT, name: "mario", channels: [] }, []).ok, false);
    assert.equal(checkDraft({ ...EMPTY_DRAFT, name: "mario", channels: ["general"] }, []).ok, true);
  });

  it("checkDraft: il prompt si conta in caratteri, non in unità UTF-16", () => {
    const prompt = "🙂".repeat(SYSTEM_PROMPT_MAX);
    assert.equal(checkDraft({ ...EMPTY_DRAFT, name: "mario", systemPrompt: prompt }, []).ok, true);
  });
});

describe("hubAvailable", () => {
  it("avvisa solo quando il backend dichiara l'HUB non disponibile", () => {
    assert.equal(hubAvailable(null), true);
    assert.equal(hubAvailable({ available: true }), true);
    assert.equal(hubAvailable({ available: false }), false);
  });

  it("l'avviso dice che l'integrazione è disabilitata", () => {
    assert.match(HUB_UNAVAILABLE_NOTICE, /momentaneamente disabilitata/);
  });
});
