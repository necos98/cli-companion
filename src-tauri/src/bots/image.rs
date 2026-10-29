//! Validazione dell'immagine di un bot: il frontend non e' fidato, quindi
//! si controllano dimensione, firma PNG, intestazione IHDR e colore.

use super::BotImage;
use base64::{engine::general_purpose::STANDARD, Engine};

/// Massimo di byte del PNG decodificato.
const IMAGE_MAX_BYTES: usize = 32 * 1024;
/// Massimo di pixel per lato.
const IMAGE_MAX_SIDE: u32 = 64;

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
const IHDR_TAG: &[u8; 4] = b"IHDR";
const IHDR_DATA_LEN: u32 = 13;
/// Offset di lunghezza, tag, larghezza, altezza e fine dell'intestazione.
const LEN_AT: usize = 8;
const TAG_AT: usize = 12;
const WIDTH_AT: usize = 16;
const HEIGHT_AT: usize = 20;
const HEADER_END: usize = 24;

const COLOR_HEX_DIGITS: usize = 6;

/// Lunghezza massima del base64 (con padding) per `IMAGE_MAX_BYTES` byte.
const fn encoded_max_len() -> usize {
    IMAGE_MAX_BYTES.div_ceil(3) * 4
}

/// Valida PNG (base64) e colore; restituisce l'immagine in forma canonica.
pub(super) fn validate_image(png: &str, color: &str) -> Result<BotImage, String> {
    let png = png.trim();
    if png.len() > encoded_max_len() {
        return Err(too_big());
    }
    let bytes = STANDARD.decode(png).map_err(|_| "immagine non valida (base64)".to_string())?;
    if bytes.is_empty() {
        return Err("immagine vuota".into());
    }
    if bytes.len() > IMAGE_MAX_BYTES {
        return Err(too_big());
    }
    check_png_header(&bytes)?;
    Ok(BotImage { png: STANDARD.encode(&bytes), color: normalize_color(color)? })
}

fn too_big() -> String {
    format!("immagine troppo grande (massimo {} KB)", IMAGE_MAX_BYTES / 1024)
}

/// Firma PNG + chunk IHDR con lati entro il massimo.
fn check_png_header(bytes: &[u8]) -> Result<(), String> {
    if !bytes.starts_with(&PNG_SIGNATURE) {
        return Err("l'immagine non è un PNG".into());
    }
    if bytes.len() < HEADER_END {
        return Err("intestazione PNG incompleta".into());
    }
    let be = |at: usize| u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    if be(LEN_AT) != IHDR_DATA_LEN || &bytes[TAG_AT..WIDTH_AT] != IHDR_TAG {
        return Err("intestazione PNG non valida".into());
    }
    let (w, h) = (be(WIDTH_AT), be(HEIGHT_AT));
    if !(1..=IMAGE_MAX_SIDE).contains(&w) || !(1..=IMAGE_MAX_SIDE).contains(&h) {
        return Err(format!("l'immagine deve essere al massimo {IMAGE_MAX_SIDE}×{IMAGE_MAX_SIDE} pixel"));
    }
    Ok(())
}

/// `#` + 6 cifre esadecimali, restituito in minuscolo.
fn normalize_color(color: &str) -> Result<String, String> {
    let hex = color.strip_prefix('#').unwrap_or("");
    if hex.len() != COLOR_HEX_DIGITS || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("colore non valido (atteso #rrggbb)".into());
    }
    Ok(color.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        let mut b = PNG_SIGNATURE.to_vec();
        b.extend_from_slice(&IHDR_DATA_LEN.to_be_bytes());
        b.extend_from_slice(IHDR_TAG);
        b.extend_from_slice(&w.to_be_bytes());
        b.extend_from_slice(&h.to_be_bytes());
        b
    }

    fn b64(bytes: &[u8]) -> String {
        STANDARD.encode(bytes)
    }

    const COLOR: &str = "#aabbcc";

    #[test]
    fn accepts_a_minimal_png_and_canonicalizes_it() {
        let bytes = png_bytes(24, 34);
        let img = validate_image(&b64(&bytes), "#AABBCC").unwrap();
        assert_eq!(img.png, b64(&bytes));
        assert_eq!(img.color, "#aabbcc");
    }

    #[test]
    fn trims_whitespace_around_the_base64() {
        let png = format!("  \n{}\n ", b64(&png_bytes(8, 8)));
        assert!(validate_image(&png, COLOR).is_ok());
    }

    #[test]
    fn rejects_non_base64_and_empty() {
        assert!(validate_image("non base64!!", COLOR).is_err());
        assert!(validate_image("", COLOR).is_err());
    }

    #[test]
    fn rejects_non_png() {
        let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10, b'J', b'F', b'I', b'F', 0, 1, 1, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0];
        assert!(validate_image(&b64(&jpeg), COLOR).is_err());
    }

    #[test]
    fn rejects_truncated_header() {
        let bytes = png_bytes(8, 8);
        assert!(validate_image(&b64(&bytes[..HEADER_END - 1]), COLOR).is_err());
        assert!(validate_image(&b64(&bytes[..PNG_SIGNATURE.len()]), COLOR).is_err());
    }

    #[test]
    fn rejects_wrong_ihdr() {
        let mut bad_tag = png_bytes(8, 8);
        bad_tag[TAG_AT..WIDTH_AT].copy_from_slice(b"IDAT");
        assert!(validate_image(&b64(&bad_tag), COLOR).is_err());
        let mut bad_len = png_bytes(8, 8);
        bad_len[LEN_AT..TAG_AT].copy_from_slice(&12u32.to_be_bytes());
        assert!(validate_image(&b64(&bad_len), COLOR).is_err());
    }

    #[test]
    fn enforces_side_limits() {
        assert!(validate_image(&b64(&png_bytes(64, 64)), COLOR).is_ok());
        assert!(validate_image(&b64(&png_bytes(1, 1)), COLOR).is_ok());
        assert!(validate_image(&b64(&png_bytes(65, 10)), COLOR).is_err());
        assert!(validate_image(&b64(&png_bytes(10, 65)), COLOR).is_err());
        assert!(validate_image(&b64(&png_bytes(0, 10)), COLOR).is_err());
        assert!(validate_image(&b64(&png_bytes(10, 0)), COLOR).is_err());
    }

    #[test]
    fn enforces_the_byte_cap() {
        let mut at_cap = png_bytes(8, 8);
        at_cap.resize(IMAGE_MAX_BYTES, 0);
        assert!(validate_image(&b64(&at_cap), COLOR).is_ok());
        at_cap.push(0);
        assert!(validate_image(&b64(&at_cap), COLOR).is_err());
        // Payload enorme: scartato prima di decodificare.
        assert!(validate_image(&"A".repeat(encoded_max_len() + 1), COLOR).is_err());
    }

    #[test]
    fn validates_the_color() {
        let png = b64(&png_bytes(8, 8));
        assert_eq!(validate_image(&png, "#abcdef").unwrap().color, "#abcdef");
        for bad in ["#abc", "abcdef", "#gggggg", "#aabbccd", "", "#", "# 12345"] {
            assert!(validate_image(&png, bad).is_err(), "{bad}");
        }
    }
}
