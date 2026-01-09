use std::path::Path;

use ed25519_dalek::pkcs8::DecodePublicKey;
use ed25519_dalek::{Signature, VerifyingKey};

pub fn verify_engine_signature(
    engine_zip: &Path,
    signature_hex: &str,
    public_key_path: &Path,
) -> Result<(), String> {
    let signature_bytes = hex::decode(signature_hex.trim())
        .map_err(|e| format!("не удалось распарсить engine signature hex: {e}"))?;

    let signature = Signature::try_from(signature_bytes.as_slice())
        .map_err(|e| format!("engine signature имеет неверную длину: {e}"))?;

    let key_pem = std::fs::read_to_string(public_key_path)
        .map_err(|e| format!("не удалось прочитать public key {}: {e}", public_key_path.display()))?;

    let key_der = decode_pem_to_der(&key_pem)
        .map_err(|e| format!("не удалось распарсить public key PEM: {e}"))?;

    let verifying_key = VerifyingKey::from_public_key_der(&key_der)
        .map_err(|e| format!("не удалось распарсить public key DER: {e}"))?;

    let engine_bytes = std::fs::read(engine_zip)
        .map_err(|e| format!("не удалось прочитать engine zip {}: {e}", engine_zip.display()))?;

    verifying_key
        .verify_strict(&engine_bytes, &signature)
        .map_err(|_| "engine signature не прошла проверку".to_string())
}

fn decode_pem_to_der(pem: &str) -> Result<Vec<u8>, String> {
    let b64: String = pem
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .filter(|l| !l.starts_with("-----BEGIN") && !l.starts_with("-----END"))
        .collect();

    if b64.is_empty() {
        return Err("PEM пустой".to_string());
    }

    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(b64.as_bytes())
        .map_err(|e| e.to_string())
}

pub fn should_allow_disable_signing_on_debug() -> bool {
    cfg!(debug_assertions)
        && std::env::var("SS14_DISABLE_SIGNING")
            .map(|v| !v.trim().is_empty() && (v == "1" || v.eq_ignore_ascii_case("true")))
            .unwrap_or(false)
}
