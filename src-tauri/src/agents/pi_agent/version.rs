//! Versione di Pi: `pi --version` stampa `1.6.0`.

use super::SUPPORTED_MAJOR;

/// Primo token che, tolta una `v` iniziale, comincia con una cifra
/// (`1.6.0`, `v1.6.0`, `pi v1.6.0` -> `1.6.0`). Tollera spazi, CRLF e righe in piu'.
pub(super) fn parse_version(out: &str) -> Option<String> {
    out.split_whitespace()
        .map(|token| token.trim_matches(|c: char| c == ',' || c == ';'))
        .map(|token| token.strip_prefix(['v', 'V']).unwrap_or(token))
        .find(|token| token.starts_with(|c: char| c.is_ascii_digit()))
        .map(str::to_string)
}

/// Versione maggiore (`1.6.0` -> 1).
fn major(version: &str) -> Option<u32> {
    version.split('.').next()?.parse().ok()
}

/// `true` se la versione rilevata ha l'API delle estensioni che usiamo.
pub(super) fn is_supported(version: Option<&str>) -> bool {
    version.and_then(major) == Some(SUPPORTED_MAJOR)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_tolerates_whitespace_crlf_prefix_and_extra_lines() {
        assert_eq!(parse_version("1.6.0").as_deref(), Some("1.6.0"));
        assert_eq!(parse_version("  1.6.0\r\n").as_deref(), Some("1.6.0"));
        assert_eq!(parse_version("v1.6.0\n").as_deref(), Some("1.6.0"));
        assert_eq!(parse_version("pi v1.6.0\nupdate available\n").as_deref(), Some("1.6.0"));
        assert_eq!(parse_version("\n\n1.2.3-beta.1\n").as_deref(), Some("1.2.3-beta.1"));
        assert_eq!(parse_version("pi"), None);
        assert_eq!(parse_version("vendor version"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn only_the_supported_major_is_supported() {
        assert!(is_supported(Some("1.6.0")));
        assert!(is_supported(Some("1.12.0-beta.1")));
        for bad in [Some("0.9.0"), Some("2.0.0"), Some("10.0.0"), Some("x.y"), Some(""), None] {
            assert!(!is_supported(bad), "{bad:?}");
        }
    }
}
