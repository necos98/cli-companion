//! `e2e-spike setup`: scarica e verifica llama.cpp, i modelli GGUF e agent-browser.
//! Idempotente: ogni passo salta cio' che e' gia' presente e valido.

mod download;
mod extract;
mod node;
mod sources;

use crate::artifacts::{GGUF_REPO, LLAMA_CPP_TAG};
use crate::llama::server::llama_server_version;
use crate::paths::Paths;
use download::{api_agent, download_agent, ensure_file, Outcome};
use std::fs;
use std::path::Path;

pub fn run(paths: &Paths) -> Result<(), String> {
    if !cfg!(windows) {
        return Err(
            "setup supporta solo Windows (gli asset di llama.cpp sono build CUDA per Windows)"
                .into(),
        );
    }
    for dir in [&paths.downloads, &paths.llama_dir, &paths.models] {
        create_dir(dir)?;
    }
    println!("[1/3] llama.cpp {LLAMA_CPP_TAG}");
    setup_llama(paths)?;
    println!("[2/3] Modelli GGUF ({GGUF_REPO})");
    setup_models(paths)?;
    println!("[3/3] agent-browser");
    let browser_version = node::install_agent_browser(paths)?;
    println!("Versioni:");
    println!(
        "  llama-server: {}",
        llama_server_version(&paths.llama_server_exe())?
    );
    println!("  agent-browser: {browser_version}");
    println!("Setup completato.");
    Ok(())
}

fn create_dir(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("creazione di {} fallita: {e}", dir.display()))
}

fn setup_llama(paths: &Paths) -> Result<(), String> {
    let assets = sources::fetch_llama_assets(&api_agent())?;
    let agent = download_agent();
    let mut downloaded_any = false;
    for asset in &assets {
        println!("  {}", asset.name);
        let dest = paths.downloads.join(&asset.name);
        let outcome = ensure_file(&agent, &asset.url, &dest, asset.sha256.as_deref())?;
        downloaded_any |= outcome == Outcome::Downloaded;
    }
    if !downloaded_any && paths.llama_server_exe().exists() {
        println!(
            "  llama-server gia' estratto in {}",
            paths.llama_dir.display()
        );
        return Ok(());
    }
    for asset in &assets {
        let count = extract::extract_zip(&paths.downloads.join(&asset.name), &paths.llama_dir)?;
        println!("  estratti {count} file da {}", asset.name);
    }
    if paths.llama_server_exe().exists() {
        Ok(())
    } else {
        Err(format!(
            "{} non trovato dopo l'estrazione",
            paths.llama_server_exe().display()
        ))
    }
}

fn setup_models(paths: &Paths) -> Result<(), String> {
    let files = sources::fetch_gguf_files(&api_agent())?;
    let agent = download_agent();
    for (quant, file) in &files {
        println!("  {}", file.name);
        let outcome = ensure_file(
            &agent,
            &file.url,
            &paths.model(*quant),
            file.sha256.as_deref(),
        )?;
        if outcome == Outcome::Present {
            println!("  gia' presente, hash verificato");
        }
    }
    Ok(())
}
