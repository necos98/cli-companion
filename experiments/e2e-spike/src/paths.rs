//! Percorsi dell'esperimento, tutti derivati dalla cartella del crate.

use crate::artifacts::{Quant, LLAMA_CPP_TAG};
use std::path::PathBuf;

pub struct Paths {
    pub root: PathBuf,
    pub downloads: PathBuf,
    pub llama_dir: PathBuf,
    pub models: PathBuf,
    pub results: PathBuf,
}

impl Paths {
    pub fn from_manifest() -> Self {
        Self::from_root(PathBuf::from(env!("CARGO_MANIFEST_DIR")))
    }

    fn from_root(root: PathBuf) -> Self {
        let cache = root.join(".cache");
        Paths {
            downloads: cache.join("downloads"),
            llama_dir: cache.join("llama").join(LLAMA_CPP_TAG),
            models: cache.join("models"),
            results: root.join("results"),
            root,
        }
    }

    pub fn llama_server_exe(&self) -> PathBuf {
        let name = if cfg!(windows) {
            "llama-server.exe"
        } else {
            "llama-server"
        };
        self.llama_dir.join(name)
    }

    pub fn model(&self, quant: Quant) -> PathBuf {
        self.models.join(quant.file_name())
    }

    pub fn agent_browser_exe(&self) -> PathBuf {
        let os = match std::env::consts::OS {
            "macos" => "darwin",
            "windows" => "win32",
            other => other,
        };
        let arch = match std::env::consts::ARCH {
            "x86_64" => "x64",
            "aarch64" => "arm64",
            other => other,
        };
        let ext = if cfg!(windows) { ".exe" } else { "" };
        self.root
            .join("node_modules")
            .join("agent-browser")
            .join("bin")
            .join(format!("agent-browser-{os}-{arch}{ext}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_is_derived_from_root() {
        let paths = Paths::from_root(PathBuf::from("root"));
        assert_eq!(paths.downloads, PathBuf::from("root/.cache/downloads"));
        assert_eq!(paths.llama_dir, PathBuf::from("root/.cache/llama/b11960"));
        assert_eq!(
            paths.model(Quant::Q8_0),
            PathBuf::from("root/.cache/models/LFM3-1.7B-Q8_0.gguf")
        );
    }

    #[test]
    fn agent_browser_exe_is_platform_specific() {
        let exe = Paths::from_root(PathBuf::from("root")).agent_browser_exe();
        let name = exe.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("agent-browser-"));
        assert_eq!(name.ends_with(".exe"), cfg!(windows));
    }
}
