//! Dove scaricare gli artefatti e con quale hash atteso (API GitHub e Hugging Face).

use crate::artifacts::{Quant, GGUF_REPO, GGUF_REVISION, LLAMA_CPP_TAG};
use serde::Deserialize;

const GITHUB_REPO: &str = "ggml-org/llama.cpp";
const CUDA_SUFFIX: &str = "bin-win-cuda-12.4-x64.zip";
const DIGEST_PREFIX: &str = "sha256:";

/// File remoto con l'hash atteso (assente se l'API non lo fornisce).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFile {
    pub name: String,
    pub url: String,
    pub sha256: Option<String>,
}

/// Nomi dei due zip di llama.cpp: build CUDA e runtime CUDA.
pub fn llama_asset_names() -> [String; 2] {
    [
        format!("llama-{LLAMA_CPP_TAG}-{CUDA_SUFFIX}"),
        format!("cudart-llama-{CUDA_SUFFIX}"),
    ]
}

#[derive(Deserialize)]
struct Release {
    assets: Vec<ReleaseAsset>,
}

#[derive(Deserialize)]
struct ReleaseAsset {
    name: String,
    digest: Option<String>,
}

pub fn fetch_llama_assets(agent: &ureq::Agent) -> Result<Vec<RemoteFile>, String> {
    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/tags/{LLAMA_CPP_TAG}");
    let release: Release = agent
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("lettura della release {LLAMA_CPP_TAG} ({url}): {e}"))?
        .body_mut()
        .read_json()
        .map_err(|e| format!("risposta della release {LLAMA_CPP_TAG} non valida: {e}"))?;
    llama_assets_from_release(&release)
}

fn llama_assets_from_release(release: &Release) -> Result<Vec<RemoteFile>, String> {
    llama_asset_names()
        .into_iter()
        .map(|name| {
            let asset = release
                .assets
                .iter()
                .find(|a| a.name == name)
                .ok_or_else(|| format!("asset {name} assente nella release {LLAMA_CPP_TAG}"))?;
            Ok(RemoteFile {
                url: format!(
                    "https://github.com/{GITHUB_REPO}/releases/download/{LLAMA_CPP_TAG}/{name}"
                ),
                sha256: asset.digest.as_deref().and_then(parse_digest),
                name,
            })
        })
        .collect()
}

/// `"sha256:<hex>"` -> `<hex>` minuscolo; altri algoritmi non sono supportati.
fn parse_digest(digest: &str) -> Option<String> {
    digest
        .strip_prefix(DIGEST_PREFIX)
        .map(str::to_ascii_lowercase)
}

#[derive(Deserialize)]
struct TreeEntry {
    path: String,
    lfs: Option<LfsInfo>,
}

#[derive(Deserialize)]
struct LfsInfo {
    oid: String,
}

pub fn fetch_gguf_files(agent: &ureq::Agent) -> Result<Vec<(Quant, RemoteFile)>, String> {
    let url = format!("https://huggingface.co/api/models/{GGUF_REPO}/tree/{GGUF_REVISION}");
    let tree: Vec<TreeEntry> = agent
        .get(&url)
        .call()
        .map_err(|e| format!("lettura dell'albero di {GGUF_REPO} ({url}): {e}"))?
        .body_mut()
        .read_json()
        .map_err(|e| format!("albero di {GGUF_REPO} non valido: {e}"))?;
    gguf_files_from_tree(&tree)
}

fn gguf_files_from_tree(tree: &[TreeEntry]) -> Result<Vec<(Quant, RemoteFile)>, String> {
    Quant::ALL
        .into_iter()
        .map(|quant| {
            let name = quant.file_name();
            let entry = tree
                .iter()
                .find(|e| e.path == name)
                .ok_or_else(|| format!("{name} assente in {GGUF_REPO}@{GGUF_REVISION}"))?;
            let sha256 = entry
                .lfs
                .as_ref()
                .map(|lfs| lfs.oid.to_ascii_lowercase())
                .ok_or_else(|| format!("{name}: hash LFS mancante nell'albero"))?;
            let file = RemoteFile {
                name: name.to_string(),
                url: format!("https://huggingface.co/{GGUF_REPO}/resolve/{GGUF_REVISION}/{name}"),
                sha256: Some(sha256),
            };
            Ok((quant, file))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(json: &str) -> Release {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn asset_names_match_the_release() {
        let [llama, cudart] = llama_asset_names();
        assert_eq!(llama, "llama-b11960-bin-win-cuda-12.4-x64.zip");
        assert_eq!(cudart, "cudart-llama-bin-win-cuda-12.4-x64.zip");
    }

    #[test]
    fn release_assets_get_urls_and_digests() {
        let json = r#"{"assets":[
            {"name":"llama-b11960-bin-win-cuda-12.4-x64.zip","digest":"sha256:ABCD"},
            {"name":"cudart-llama-bin-win-cuda-12.4-x64.zip","digest":null},
            {"name":"altro.zip","digest":"sha256:ff"}]}"#;
        let files = llama_assets_from_release(&release(json)).unwrap();
        assert_eq!(files[0].sha256.as_deref(), Some("abcd"));
        assert_eq!(files[1].sha256, None);
        assert_eq!(
            files[0].url,
            "https://github.com/ggml-org/llama.cpp/releases/download/b11960/llama-b11960-bin-win-cuda-12.4-x64.zip"
        );
    }

    #[test]
    fn missing_asset_is_an_error() {
        let err = llama_assets_from_release(&release(r#"{"assets":[]}"#)).unwrap_err();
        assert!(err.contains("assente"), "{err}");
    }

    #[test]
    fn unknown_digest_algorithm_is_ignored() {
        assert_eq!(parse_digest("sha512:aa"), None);
        assert_eq!(parse_digest("sha256:AA").as_deref(), Some("aa"));
    }

    #[test]
    fn gguf_tree_yields_hashes_per_quant() {
        let json = r#"[
            {"path":"README.md","size":10},
            {"path":"LFM3-1.7B-Q8_0.gguf","size":1,"lfs":{"oid":"AA"}},
            {"path":"LFM3-1.7B-Q4_K_M.gguf","size":1,"lfs":{"oid":"bb"}}]"#;
        let tree: Vec<TreeEntry> = serde_json::from_str(json).unwrap();
        let files = gguf_files_from_tree(&tree).unwrap();
        assert_eq!(files[0].0, Quant::Q8_0);
        assert_eq!(files[0].1.sha256.as_deref(), Some("aa"));
        assert!(files[1].1.url.ends_with("/LFM3-1.7B-Q4_K_M.gguf"));
    }

    #[test]
    fn gguf_without_lfs_hash_is_an_error() {
        let tree: Vec<TreeEntry> = serde_json::from_str(
            r#"[{"path":"LFM3-1.7B-Q8_0.gguf"},{"path":"LFM3-1.7B-Q4_K_M.gguf"}]"#,
        )
        .unwrap();
        assert!(gguf_files_from_tree(&tree).is_err());
    }
}
