//! Metadati della run: versioni degli strumenti, modelli con hash, argomenti.

use super::config::RunConfig;
use super::RunArgs;
use crate::artifacts::{Quant, AGENT_BROWSER_VERSION, GGUF_REPO, GGUF_REVISION, LLAMA_CPP_TAG};
use crate::llama::server::llama_server_version;
use crate::paths::Paths;
use crate::proc::output_with_timeout;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

const VERSION_TIMEOUT: Duration = Duration::from_secs(15);
const HASH_BUFFER_BYTES: usize = 1 << 20;

pub struct Meta {
    pub started_at: String,
    llama_server: String,
    agent_browser: String,
    models: Vec<Value>,
}

impl Meta {
    /// Raccoglie le versioni e gli hash dei modelli usati dalle configurazioni scelte.
    pub fn collect(
        paths: &Paths,
        configs: &[RunConfig],
        started_at: String,
    ) -> Result<Self, String> {
        let mut quants: Vec<Quant> = Vec::new();
        for config in configs {
            if !quants.contains(&config.quant) {
                quants.push(config.quant);
            }
        }
        Ok(Meta {
            started_at,
            llama_server: llama_server_version(&paths.llama_server_exe())
                .unwrap_or_else(|e| format!("sconosciuta ({e})")),
            agent_browser: agent_browser_version(&paths.agent_browser_exe())?,
            models: quants.into_iter().map(|q| model_entry(paths, q)).collect(),
        })
    }

    pub fn to_json(&self, args: &RunArgs, finished_at: Option<String>) -> Value {
        json!({
            "started_at": self.started_at,
            "finished_at": finished_at,
            "llama_cpp_tag": LLAMA_CPP_TAG,
            "llama_server_version": self.llama_server,
            "agent_browser_expected": AGENT_BROWSER_VERSION,
            "agent_browser_version": self.agent_browser,
            "gguf_repo": GGUF_REPO,
            "gguf_revision": GGUF_REVISION,
            "models": self.models,
            "args": {
                "configs": args.configs,
                "scenarios": args.scenarios,
                "runs": args.runs,
                "max_actions": args.max_actions,
                "llama_port": args.llama_port,
                "fixture_port": args.fixture_port,
                "out": args.out,
                "headed": args.headed,
            },
        })
    }
}

fn agent_browser_version(exe: &Path) -> Result<String, String> {
    let output = output_with_timeout(Command::new(exe).arg("--version"), VERSION_TIMEOUT)?;
    if !output.status.success() {
        return Err(format!("{} --version e' fallito", exe.display()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn model_entry(paths: &Paths, quant: Quant) -> Value {
    let file = paths.model(quant);
    let sha256 = sha256_file(&file).unwrap_or_else(|e| format!("non calcolabile: {e}"));
    json!({ "quant": quant.label(), "file": quant.file_name(), "sha256": sha256 })
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; HASH_BUFFER_BYTES];
    loop {
        let read = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_a_file_in_chunks() {
        let path = std::env::temp_dir().join(format!("e2e-spike-hash-{}.bin", std::process::id()));
        std::fs::write(&path, b"abc").unwrap();
        let hash = sha256_file(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            hash,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn missing_file_is_an_error() {
        assert!(sha256_file(Path::new("definitely/missing.gguf")).is_err());
    }
}
