//! Estrazione di zip con protezione dal zip-slip.

use std::fs::{self, File};
use std::io;
use std::path::Path;

/// Estrae tutto lo zip in `dest`; ritorna il numero di file scritti.
pub fn extract_zip(zip_path: &Path, dest: &Path) -> Result<usize, String> {
    let file = File::open(zip_path)
        .map_err(|e| format!("apertura di {} fallita: {e}", zip_path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("{} non e' uno zip valido: {e}", zip_path.display()))?;
    fs::create_dir_all(dest)
        .map_err(|e| format!("creazione di {} fallita: {e}", dest.display()))?;
    let mut written = 0;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|e| format!("{}: voce {index}: {e}", zip_path.display()))?;
        let relative = entry.enclosed_name().ok_or_else(|| {
            format!(
                "{}: percorso non sicuro '{}'",
                zip_path.display(),
                entry.name()
            )
        })?;
        let target = dest.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&target)
                .map_err(|e| format!("creazione di {} fallita: {e}", target.display()))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("creazione di {} fallita: {e}", parent.display()))?;
        }
        let mut out = File::create(&target)
            .map_err(|e| format!("creazione di {} fallita: {e}", target.display()))?;
        io::copy(&mut entry, &mut out)
            .map_err(|e| format!("estrazione in {} fallita: {e}", target.display()))?;
        written += 1;
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("e2e-spike-zip-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let mut writer = zip::ZipWriter::new(File::create(path).unwrap());
        for (name, data) in entries {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap();
    }

    #[test]
    fn extracts_flat_and_nested_files() {
        let dir = scratch("ok");
        let zip_path = dir.join("a.zip");
        make_zip(
            &zip_path,
            &[("llama-server.exe", b"exe"), ("sub/x.dll", b"dll")],
        );
        let out = dir.join("out");
        assert_eq!(extract_zip(&zip_path, &out).unwrap(), 2);
        assert_eq!(fs::read(out.join("llama-server.exe")).unwrap(), b"exe");
        assert_eq!(fs::read(out.join("sub").join("x.dll")).unwrap(), b"dll");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_zip_slip() {
        let dir = scratch("slip");
        let zip_path = dir.join("evil.zip");
        make_zip(&zip_path, &[("../evil.txt", b"x")]);
        let err = extract_zip(&zip_path, &dir.join("out")).unwrap_err();
        assert!(err.contains("non sicuro"), "{err}");
        assert!(!dir.join("evil.txt").exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
