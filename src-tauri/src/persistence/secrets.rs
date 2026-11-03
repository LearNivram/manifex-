//! Encrypted secret storage. Plaintext never touches the database: values are
//! encrypted by the [`SecretVault`] (key held by the OS credential store).

use rusqlite::{params, OptionalExtension};

use super::Db;
use crate::error::{AppError, AppResult};
use crate::security::vault::{EncryptedSecret, SecretVault};

impl Db {
    pub fn put_secret(&self, vault: &SecretVault, id: &str, plaintext: &[u8]) -> AppResult<()> {
        let enc = vault.encrypt(id, plaintext)?;
        let now = chrono::Utc::now().to_rfc3339();
        self.with(|c| {
            c.execute(
                "INSERT INTO secrets (id, nonce, ciphertext, created_at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(id) DO UPDATE SET nonce=excluded.nonce, ciphertext=excluded.ciphertext",
                params![id, enc.nonce, enc.ciphertext, now],
            )
        })?;
        Ok(())
    }

    pub fn get_secret(&self, vault: &SecretVault, id: &str) -> AppResult<Vec<u8>> {
        let row: Option<(Vec<u8>, Vec<u8>)> = self.with(|c| {
            c.query_row("SELECT nonce, ciphertext FROM secrets WHERE id=?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()
        })?;
        let (nonce, ciphertext) = row.ok_or_else(|| AppError::NotFound(format!("secret {id}")))?;
        vault.decrypt(id, &EncryptedSecret { nonce, ciphertext })
    }

    pub fn delete_secret(&self, id: &str) -> AppResult<()> {
        self.with(|c| c.execute("DELETE FROM secrets WHERE id=?1", [id]))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_only_ciphertext() {
        let db = Db::open_in_memory().unwrap();
        let vault = SecretVault::with_key([3u8; 32]);
        db.put_secret(&vault, "s1", b"my-token").unwrap();
        let raw: Vec<u8> = db.with(|c| c.query_row("SELECT ciphertext FROM secrets WHERE id='s1'", [], |r| r.get(0))).unwrap();
        assert!(!raw.windows(8).any(|w| w == b"my-token"));
        assert_eq!(db.get_secret(&vault, "s1").unwrap(), b"my-token");
        db.delete_secret("s1").unwrap();
        assert!(db.get_secret(&vault, "s1").is_err());
    }
}
