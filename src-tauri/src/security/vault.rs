//! Secret vault backed by the operating system credential store.
//!
//! Design (envelope encryption):
//! * A random 256-bit master key is generated once and stored in the native
//!   credential store (Windows Credential Manager, macOS Keychain, Linux Secret
//!   Service) via the `keyring` crate.
//! * Individual secrets (tokens, client keys, imported kubeconfigs) are encrypted
//!   with XChaCha20-Poly1305 using that key and stored as ciphertext in SQLite. The
//!   secret id is bound as associated data so ciphertexts cannot be swapped.
//!
//! This avoids the size limits of some native stores (Windows: 2.5 KB per entry,
//! too small for kubeconfigs with embedded certificates) while guaranteeing that no
//! credential is ever persisted in plaintext. Without access to the OS store the
//! vault refuses to persist anything.

use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};
use parking_lot::Mutex;
use rand::RngCore;
use zeroize_shim::Zeroizing;

use crate::error::{AppError, AppResult};

const KEYRING_SERVICE: &str = "dev.manifex.desktop";
const KEYRING_ACCOUNT: &str = "vault-master-key-v1";

/// Minimal zeroizing wrapper so key material is wiped from memory on drop.
mod zeroize_shim {
    use std::ops::{Deref, DerefMut};

    pub struct Zeroizing<T: AsMut<[u8]>>(pub T);

    impl<T: AsMut<[u8]>> Drop for Zeroizing<T> {
        fn drop(&mut self) {
            for b in self.0.as_mut().iter_mut() {
                // volatile write prevents the compiler from eliding the wipe
                unsafe { std::ptr::write_volatile(b, 0) };
            }
        }
    }

    impl<T: AsMut<[u8]>> Deref for Zeroizing<T> {
        type Target = T;
        fn deref(&self) -> &T {
            &self.0
        }
    }

    impl<T: AsMut<[u8]>> DerefMut for Zeroizing<T> {
        fn deref_mut(&mut self) -> &mut T {
            &mut self.0
        }
    }
}

pub struct EncryptedSecret {
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

enum KeySource {
    /// Key is loaded lazily from the OS credential store.
    Keyring,
    /// Caller-provided key (tests and headless tooling only).
    Fixed,
}

pub struct SecretVault {
    source: KeySource,
    key: Mutex<Option<Zeroizing<[u8; 32]>>>,
}

impl SecretVault {
    pub fn os_backed() -> Self {
        Self {
            source: KeySource::Keyring,
            key: Mutex::new(None),
        }
    }

    /// Creates a vault with a caller-provided key. Intended for automated tests and
    /// headless tooling; the desktop application always uses [`Self::os_backed`].
    pub fn with_key(key: [u8; 32]) -> Self {
        Self {
            source: KeySource::Fixed,
            key: Mutex::new(Some(Zeroizing(key))),
        }
    }

    /// Reports whether the native credential store is reachable.
    pub fn status(&self) -> Result<(), String> {
        let mut guard = self.key.lock();
        if guard.is_some() {
            return Ok(());
        }
        match self.load_or_create_key() {
            Ok(k) => {
                *guard = Some(k);
                Ok(())
            }
            Err(e) => Err(e.to_string()),
        }
    }

    fn load_or_create_key(&self) -> AppResult<Zeroizing<[u8; 32]>> {
        match self.source {
            KeySource::Keyring => {}
            KeySource::Fixed => return Err(AppError::Vault("fixed key missing".into())),
        }
        use base64::Engine;
        let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)
            .map_err(|e| AppError::Vault(e.to_string()))?;
        match entry.get_password() {
            Ok(encoded) => {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(encoded.trim())
                    .map_err(|_| AppError::Vault("master key is corrupt".into()))?;
                if bytes.len() != 32 {
                    return Err(AppError::Vault("master key has invalid length".into()));
                }
                let mut key = Zeroizing([0u8; 32]);
                key.copy_from_slice(&bytes);
                Ok(key)
            }
            Err(keyring::Error::NoEntry) => {
                let mut key = Zeroizing([0u8; 32]);
                rand::rngs::OsRng.fill_bytes(&mut key[..]);
                let encoded = base64::engine::general_purpose::STANDARD.encode(&key[..]);
                entry
                    .set_password(&encoded)
                    .map_err(|e| AppError::Vault(e.to_string()))?;
                tracing::info!("created new vault master key in OS credential store");
                Ok(key)
            }
            Err(e) => Err(AppError::Vault(e.to_string())),
        }
    }

    fn with_cipher<T>(&self, f: impl FnOnce(&XChaCha20Poly1305) -> AppResult<T>) -> AppResult<T> {
        let mut guard = self.key.lock();
        if guard.is_none() {
            *guard = Some(self.load_or_create_key()?);
        }
        let key = guard.as_ref().expect("key loaded");
        let cipher = XChaCha20Poly1305::new_from_slice(&key[..])
            .map_err(|_| AppError::Vault("invalid key".into()))?;
        f(&cipher)
    }

    pub fn encrypt(&self, secret_id: &str, plaintext: &[u8]) -> AppResult<EncryptedSecret> {
        self.with_cipher(|cipher| {
            let mut nonce = [0u8; 24];
            rand::rngs::OsRng.fill_bytes(&mut nonce);
            let ciphertext = cipher
                .encrypt(
                    XNonce::from_slice(&nonce),
                    Payload {
                        msg: plaintext,
                        aad: secret_id.as_bytes(),
                    },
                )
                .map_err(|_| AppError::Vault("encryption failed".into()))?;
            Ok(EncryptedSecret {
                nonce: nonce.to_vec(),
                ciphertext,
            })
        })
    }

    pub fn decrypt(&self, secret_id: &str, secret: &EncryptedSecret) -> AppResult<Vec<u8>> {
        if secret.nonce.len() != 24 {
            return Err(AppError::Vault("invalid nonce".into()));
        }
        self.with_cipher(|cipher| {
            cipher
                .decrypt(
                    XNonce::from_slice(&secret.nonce),
                    Payload {
                        msg: &secret.ciphertext,
                        aad: secret_id.as_bytes(),
                    },
                )
                .map_err(|_| {
                    AppError::Vault(
                        "decryption failed – secret was tampered with or key changed".into(),
                    )
                })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let v = SecretVault::with_key([7u8; 32]);
        let enc = v.encrypt("id-1", b"super-secret-token").unwrap();
        assert_ne!(enc.ciphertext, b"super-secret-token");
        assert_eq!(v.decrypt("id-1", &enc).unwrap(), b"super-secret-token");
    }

    #[test]
    fn aad_binding_prevents_swapping() {
        let v = SecretVault::with_key([7u8; 32]);
        let enc = v.encrypt("id-1", b"token").unwrap();
        assert!(v.decrypt("id-2", &enc).is_err());
    }

    #[test]
    fn tamper_detection() {
        let v = SecretVault::with_key([1u8; 32]);
        let mut enc = v.encrypt("x", b"token").unwrap();
        enc.ciphertext[0] ^= 0xff;
        assert!(v.decrypt("x", &enc).is_err());
    }
}
