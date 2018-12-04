#![feature(nll)]
mod errors;
mod shared;

use self::shared::{Keys, Vault};
use crate::errors::Error;
use openssl::rand;
use serde_derive::{Deserialize, Serialize};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Used to specify where encryption/decryption keys should be loaded from
pub enum KeySource<'a> {
    /// Load the keys from a binary file on-disk
    File(&'a Path),
    /// Derive keys from the specified password
    Password(&'a str),
    /// Generate new keys from a secure RNG
    Generate
}

/// The primary interface used for interacting with the SecureStore.
pub struct SecretsManager {
    vault: Vault,
    path: PathBuf,
    keys: Keys,
}

impl SecretsManager {
    /// Creates a new vault on-disk at path `p` and loads it in a new instance
    /// of `SecretsManager`. A newly created store is initialized with randomly-
    /// generated encryption keys and may be used immediately, or the default keys
    /// may be overridden with [`SecretsManager::load_keys`].
    pub fn new<P: AsRef<Path>>(path: P, key_source: KeySource) -> Result<Self, Error> {
        let path = path.as_ref();

        let vault = Vault::from_file(path)?;
        Ok(SecretsManager {
            keys: key_source.extract_keys(&vault.iv)?,
            vault,
            path: PathBuf::from(path),
        })
    }

    /// Creates a new instance of `SecretsManager` referencing an existing vault
    /// located on-disk.
    pub fn load<P: AsRef<Path>>(path: P, key_source: KeySource) -> Result<Self, Error> {
        let path = path.as_ref();

        let vault = Vault::from_file(path)?;
        Ok(SecretsManager {
            keys: key_source.extract_keys(&vault.iv)?,
            vault,
            path: PathBuf::from(path),
        })
    }

    /// Saves changes to the underlying vault specified by the path supplied during
    /// construction of this `SecretsManager` instance.
    pub fn save(&self) -> Result<(), Error> {
        self.vault.save(&self.path)
    }

    /// Exports the private key(s) resident in memory to a path on-disk. Note that
    /// in addition to be used to export (existing) keys previously loaded into the
    /// secrets store and (new) keys generated by the secrets store, it can also be
    /// used to export keys (possibly interactively) derived from passwords to an
    /// equivalent representation on-disk.
    pub fn export_keys<P: AsRef<Path>>(&self, path: P) -> Result<(), Error> {
        self.keys.export(path)
    }
}

impl<'a> KeySource<'a> {
    fn extract_keys(&self, iv: &Option<[u8; shared::IV_SIZE]>) -> Result<Keys, Error> {
        let mut encryption_key = [0u8; shared::KEY_LENGTH];
        let mut hmac_key = [0u8; shared::KEY_LENGTH];

        match &Self {
            KeySource::Generate => {
                rand::rand_bytes(&mut encryption_key)
                    .expect("Key generation failure!");
                rand::rand_bytes(&mut hmac_key)
                    .expect("Key generation failure!");
            },
            KeySource::File(path) => {
                let mut file = File::open(path)
                    .map_err(Error::Io)?;

                file.read_exact(&mut encryption_key)
                    .map_err(Error::Io)?;
                file.read_exact(&mut hmac_key)
                    .map_err(Error::Io)?;
            },
            KeySource::Password(password) => {
                use openssl::pkcs5::pbkdf2_hmac;
                use openssl::hash::MessageDigest;

                let iv = match iv {
                    None => return Err(Error::MissingVaultIV),
                    Some(x) => x,
                };

                let mut key_data = [0u8; shared::KEY_COUNT * shared::KEY_LENGTH];
                pbkdf2_hmac(password.as_bytes(), iv, shared::PBKDF2_ROUNDS, MessageDigest::sha1(), &mut key_data)
                    .expect("PBKDF2 key generation failed!");

                encryption_key.copy_from_slice(&key_data[0*shared::KEY_LENGTH..1*shared::KEY_LENGTH]);
                hmac_key.copy_from_slice(&key_data[1*shared::KEY_LENGTH..2*shared::KEY_LENGTH]);
            }
        };

        Ok(Keys {
            encryption: encryption_key,
            hmac: hmac_key,
        })
    }
}