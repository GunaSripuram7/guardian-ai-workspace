// crates/guardian-protection/src/rollback/vault.rs
// AES-256-GCM encrypted vault for file snapshots.
// Format of each vault file: [12-byte nonce][ciphertext]

use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use std::path::PathBuf;
use std::fs;

pub struct VaultManager {
    pub vault_dir: PathBuf,
    key:           Key<Aes256Gcm>,
}

impl VaultManager {
    /// Create or open the vault. Key is generated once and stored in vault_dir/.key
    pub fn new(vault_dir: &str) -> Result<Self, String> {
        let dir = PathBuf::from(vault_dir);
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let key_path = dir.join(".key");
        let key_bytes: Vec<u8> = if key_path.exists() {
            fs::read(&key_path).map_err(|e| e.to_string())?
        } else {
            let new_key = Aes256Gcm::generate_key(OsRng);
            fs::write(&key_path, new_key.as_slice()).map_err(|e| e.to_string())?;
            new_key.as_slice().to_vec()
        };
        let key = *Key::<Aes256Gcm>::from_slice(&key_bytes);
        Ok(Self { vault_dir: dir, key })
    }

    /// Encrypt a file and store it in the vault. Returns the vault file path.
    pub fn store(&self, snapshot_id: &str, plaintext: &[u8]) -> Result<String, String> {
        let cipher = Aes256Gcm::new(&self.key);
        let nonce  = Aes256Gcm::generate_nonce(&mut OsRng);
        let ciphertext = cipher.encrypt(&nonce, plaintext)
            .map_err(|e| format!("Encryption failed: {}", e))?;
        // vault file = nonce (12 bytes) || ciphertext
        let mut vault_bytes = nonce.to_vec();
        vault_bytes.extend(ciphertext);
        let vault_path = self.vault_dir.join(format!("{}.vault", snapshot_id));
        fs::write(&vault_path, &vault_bytes).map_err(|e| e.to_string())?;
        Ok(vault_path.to_string_lossy().to_string())
    }

    /// Decrypt and return the plaintext of a vault file.
    pub fn retrieve(&self, vault_path: &str) -> Result<Vec<u8>, String> {
        let vault_bytes = fs::read(vault_path).map_err(|e| e.to_string())?;
        if vault_bytes.len() < 12 {
            return Err("Vault file too short — corrupt?".to_string());
        }
        let (nonce_bytes, ciphertext) = vault_bytes.split_at(12);
        let cipher = Aes256Gcm::new(&self.key);
        let nonce  = Nonce::from_slice(nonce_bytes);
        cipher.decrypt(nonce, ciphertext)
            .map_err(|e| format!("Decryption failed: {}", e))
    }

    /// Delete a vault file (called during purge of expired snapshots).
    pub fn delete(&self, vault_path: &str) {
        let _ = fs::remove_file(vault_path);
    }
}
