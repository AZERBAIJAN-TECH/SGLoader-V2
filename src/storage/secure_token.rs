#[cfg(target_os = "windows")]
mod win {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    };
    use windows::core::PCWSTR;

    pub fn encrypt_token(bytes: &[u8]) -> Result<Vec<u8>, String> {
        unsafe {
            let in_blob = CRYPT_INTEGER_BLOB {
                cbData: bytes.len() as u32,
                pbData: bytes.as_ptr() as *mut u8,
            };

            let mut out_blob = CRYPT_INTEGER_BLOB::default();

            CryptProtectData(
                &in_blob,
                PCWSTR::null(),
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out_blob,
            )
            .map_err(|e| format!("DPAPI protect error: {e}"))?;

            let data =
                std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize).to_vec();
            LocalFree(HLOCAL(out_blob.pbData as *mut core::ffi::c_void));
            Ok(data)
        }
    }

    pub fn decrypt_token(bytes: &[u8]) -> Result<String, String> {
        unsafe {
            let in_blob = CRYPT_INTEGER_BLOB {
                cbData: bytes.len() as u32,
                pbData: bytes.as_ptr() as *mut u8,
            };
            let mut out_blob = CRYPT_INTEGER_BLOB::default();

            CryptUnprotectData(
                &in_blob,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out_blob,
            )
            .map_err(|e| format!("DPAPI unprotect error: {e}"))?;

            let data =
                std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize).to_vec();
            LocalFree(HLOCAL(out_blob.pbData as *mut core::ffi::c_void));

            String::from_utf8(data).map_err(|e| format!("DPAPI data is not UTF-8: {e}"))
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod win {
    pub fn encrypt_token(bytes: &[u8]) -> Result<Vec<u8>, String> {
        // On non-Windows platforms we just persist the token as-is.
        // This keeps login persistence working even without a platform key store.
        Ok(bytes.to_vec())
    }

    pub fn decrypt_token(bytes: &[u8]) -> Result<String, String> {
        String::from_utf8(bytes.to_vec()).map_err(|e| format!("token is not UTF-8: {e}"))
    }
}

pub use win::{decrypt_token, encrypt_token};
