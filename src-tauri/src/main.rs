#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn print_help() {
    println!(
        "cli-companion {}\n\
         \n\
         Uso:\n  \
           cli-companion                 Avvia l'interfaccia grafica\n  \
           cli-companion --check-update  Controlla le release su GitHub ed esce\n  \
           cli-companion --check-update --json  Come sopra, output JSON\n  \
           cli-companion --notify <evento> [agente]  Segnala un evento di un coding\n  \
             agent (session-start|session-end|finished|needs-input|test) all'istanza in esecuzione ed\n  \
             esce; legge il JSON dell'hook da stdin e l'id tab da CLI_COMPANION_SESSION\n  \
             (eventi non riconosciuti o senza tab: ignorati, exit 0)\n  \
           cli-companion --mcp           Server MCP (stdio) con i tool per orchestrare sessioni\n  \
             automatiche; lanciato da Claude Code, usa CLI_COMPANION_SESSION e\n  \
             CLI_COMPANION_TOKEN (fuori da cli-companion espone zero tool)\n  \
           cli-companion --version      Stampa la versione ed esce\n  \
           cli-companion --help          Questo aiuto\n\
         \n\
         Exit code di --check-update: 0 = aggiornato, 2 = aggiornamento disponibile,\n  \
         1 = errore (repo non configurato, rete, ...).\n\
         \n\
         Nota Windows: le build release non hanno console (windows_subsystem), quindi\n  \
         --check-update/--version sono pensati per le build debug o per script;\n  \
         --notify invece non stampa nulla e funziona anche in release: e' il\n  \
         comando usato dagli hook dei coding agent;\n         --mcp parla solo JSON-RPC su stdio.",
        cli_companion_lib::updater::current_version()
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("cli-companion {}", cli_companion_lib::updater::current_version());
        return;
    }
    if let Some(pos) = args.iter().position(|a| a == "--notify") {
        let event = args.get(pos + 1).cloned().unwrap_or_default();
        let agent = args.get(pos + 2).cloned().unwrap_or("local".to_string());
        match cli_companion_lib::agent_notify::run_notify(&event, &agent) {
            Ok(()) => std::process::exit(0),
            Err(e) => {
                eprintln!("notify fallita: {e}");
                std::process::exit(1);
            }
        }
    }
    if args.iter().any(|a| a == "--mcp") {
        cli_companion_lib::mcp::run_stdio();
        std::process::exit(0);
    }
    if args.iter().any(|a| a == "--check-update") {
        let as_json = args.iter().any(|a| a == "--json");
        match cli_companion_lib::updater::check_blocking() {
            Ok(st) => {
                if as_json {
                    println!("{}", serde_json::to_string_pretty(&st).unwrap_or_default());
                } else if st.update_available {
                    println!(
                        "Aggiornamento disponibile: {} -> {} ({})",
                        st.current_version, st.latest_version, st.release_url
                    );
                } else {
                    println!("cli-companion aggiornato ({}).", st.current_version);
                }
                std::process::exit(if st.update_available { 2 } else { 0 });
            }
            Err(e) => {
                eprintln!("Controllo aggiornamenti fallito: {e}");
                std::process::exit(1);
            }
        }
    }
    cli_companion_lib::run();
}
