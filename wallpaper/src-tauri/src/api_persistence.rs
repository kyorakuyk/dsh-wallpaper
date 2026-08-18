//! Current-user encrypted persistence for DeepSeek API transcripts.
//!
//! This module deliberately owns only opaque JSON persistence.  Conversation
//! validation remains in `chat`, while this layer guarantees that a transcript
//! is never written to disk in plaintext and that a corrupt ciphertext is not
//! silently replaced with an empty archive.

use serde::{de::DeserializeOwned, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const MAX_ENCRYPTED_BYTES: u64 = 32 * 1024 * 1024;
const MAX_PLAINTEXT_BYTES: usize = 16 * 1024 * 1024;
static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Errors intentionally omit file contents, ciphertext and transcript text.
/// They are safe to surface as a generic diagnostic, but are not a reason to
/// replace the existing archive.
#[derive(Debug)]
pub enum PersistenceError {
    Unavailable,
    InvalidArchive,
    Io,
    Serialization,
    Encryption,
    Decryption,
}

impl std::fmt::Display for PersistenceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::Unavailable => "当前平台不支持受保护的 API 会话存储",
            Self::InvalidArchive => "加密 API 会话记录格式无效",
            Self::Io => "无法安全读写加密 API 会话记录",
            Self::Serialization => "无法序列化 API 会话记录",
            Self::Encryption => "无法加密 API 会话记录",
            Self::Decryption => "无法解密 API 会话记录",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for PersistenceError {}

#[derive(Clone, Debug)]
pub struct EncryptedJsonStore {
    path: PathBuf,
}

impl EncryptedJsonStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns `None` only when no archive exists yet.  A damaged, symlinked,
    /// oversized, or undecryptable archive is an error so callers can stop
    /// writing rather than overwrite it.
    pub fn load<T: DeserializeOwned>(&self) -> Result<Option<T>, PersistenceError> {
        let ciphertext = match self.read_ciphertext()? {
            Some(ciphertext) => ciphertext,
            None => return Ok(None),
        };
        let plaintext = dpapi_unprotect(&ciphertext)?;
        if plaintext.len() > MAX_PLAINTEXT_BYTES {
            return Err(PersistenceError::InvalidArchive);
        }
        serde_json::from_slice(&plaintext)
            .map(Some)
            .map_err(|_| PersistenceError::InvalidArchive)
    }

    /// Writes to a same-directory temporary file, syncs it, then atomically
    /// replaces the old archive.  Every failure before replacement leaves the
    /// previous ciphertext untouched.
    pub fn save<T: Serialize>(&self, value: &T) -> Result<(), PersistenceError> {
        let plaintext = serde_json::to_vec(value).map_err(|_| PersistenceError::Serialization)?;
        if plaintext.len() > MAX_PLAINTEXT_BYTES {
            return Err(PersistenceError::InvalidArchive);
        }
        let ciphertext = dpapi_protect(&plaintext)?;
        self.write_ciphertext_atomically(&ciphertext)
    }

    fn read_ciphertext(&self) -> Result<Option<Vec<u8>>, PersistenceError> {
        let metadata = match fs::symlink_metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(PersistenceError::Io),
        };
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() == 0
            || metadata.len() > MAX_ENCRYPTED_BYTES
        {
            return Err(PersistenceError::InvalidArchive);
        }
        fs::read(&self.path)
            .map(Some)
            .map_err(|_| PersistenceError::Io)
    }

    fn write_ciphertext_atomically(&self, ciphertext: &[u8]) -> Result<(), PersistenceError> {
        let parent = self.path.parent().ok_or(PersistenceError::Io)?;
        fs::create_dir_all(parent).map_err(|_| PersistenceError::Io)?;
        if let Ok(metadata) = fs::symlink_metadata(&self.path) {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(PersistenceError::InvalidArchive);
            }
        }
        let (temporary, mut file) = self.create_temporary_file(parent)?;
        let write_result = (|| {
            file.write_all(ciphertext)
                .map_err(|_| PersistenceError::Io)?;
            file.sync_all().map_err(|_| PersistenceError::Io)?;
            drop(file);
            if self.path.exists() {
                atomic_replace_existing(&temporary, &self.path)
            } else {
                atomic_install_new(&temporary, &self.path)
            }
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        write_result
    }

    fn next_temporary_path(&self, parent: &Path) -> Result<PathBuf, PersistenceError> {
        let file_name = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(PersistenceError::Io)?;
        // We do not create the file here, so an extremely unlikely collision
        // is dealt with by `create_new` rather than following a hostile file.
        let nonce = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        Ok(parent.join(format!(".{file_name}.{}.{}.tmp", std::process::id(), nonce)))
    }

    fn create_temporary_file(
        &self,
        parent: &Path,
    ) -> Result<(PathBuf, std::fs::File), PersistenceError> {
        // `create_new` refuses an existing pathname, including a symlink.  A
        // bounded retry handles a collision without accepting an attacker- or
        // stale-process-controlled temporary file.
        for _ in 0..16 {
            let temporary = self.next_temporary_path(parent)?;
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
            {
                Ok(file) => return Ok((temporary, file)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(PersistenceError::Io),
            }
        }
        Err(PersistenceError::Io)
    }
}

#[cfg(windows)]
fn atomic_replace_existing(temporary: &Path, destination: &Path) -> Result<(), PersistenceError> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        core::PCWSTR,
        Win32::Storage::FileSystem::{ReplaceFileW, REPLACEFILE_WRITE_THROUGH},
    };

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }
    let temporary = wide(temporary);
    let destination = wide(destination);
    unsafe {
        // ReplaceFileW is explicitly an atomic replacement of an existing
        // destination. It never asks us to delete the prior ciphertext first.
        // WRITE_THROUGH requests the metadata flush before success is reported.
        ReplaceFileW(
            PCWSTR(destination.as_ptr()),
            PCWSTR(temporary.as_ptr()),
            PCWSTR::null(),
            REPLACEFILE_WRITE_THROUGH,
            None,
            None,
        )
    }
    .map_err(|_| PersistenceError::Io)
}

#[cfg(not(windows))]
fn atomic_replace_existing(temporary: &Path, destination: &Path) -> Result<(), PersistenceError> {
    fs::rename(temporary, destination).map_err(|_| PersistenceError::Io)
}

#[cfg(windows)]
fn atomic_install_new(temporary: &Path, destination: &Path) -> Result<(), PersistenceError> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        core::PCWSTR,
        Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH},
    };

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }
    let temporary = wide(temporary);
    let destination = wide(destination);
    unsafe {
        // No REPLACE_EXISTING: if another process creates the destination in
        // the race, fail rather than overwrite a ciphertext we did not read.
        MoveFileExW(
            PCWSTR(temporary.as_ptr()),
            PCWSTR(destination.as_ptr()),
            MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|_| PersistenceError::Io)
}

#[cfg(not(windows))]
fn atomic_install_new(temporary: &Path, destination: &Path) -> Result<(), PersistenceError> {
    fs::rename(temporary, destination).map_err(|_| PersistenceError::Io)
}

#[cfg(windows)]
fn dpapi_protect(plaintext: &[u8]) -> Result<Vec<u8>, PersistenceError> {
    use windows::{
        core::w,
        Win32::{
            Foundation::{LocalFree, HLOCAL},
            Security::Cryptography::{
                CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
            },
        },
    };

    const ENTROPY: &[u8] = b"dsh-wallpaper/api-conversations/v1";

    struct LocalBlob(CRYPT_INTEGER_BLOB);
    impl Default for LocalBlob {
        fn default() -> Self {
            Self(CRYPT_INTEGER_BLOB::default())
        }
    }
    impl LocalBlob {
        fn as_bytes(&self) -> &[u8] {
            if self.0.cbData == 0 {
                &[]
            } else {
                debug_assert!(!self.0.pbData.is_null());
                // DPAPI owns this allocation until `LocalBlob::drop`.
                unsafe { std::slice::from_raw_parts(self.0.pbData, self.0.cbData as usize) }
            }
        }
    }
    impl Drop for LocalBlob {
        fn drop(&mut self) {
            if !self.0.pbData.is_null() {
                // DPAPI allocates returned blobs with LocalAlloc.
                unsafe {
                    let _ = LocalFree(Some(HLOCAL(self.0.pbData.cast())));
                }
                self.0.pbData = std::ptr::null_mut();
                self.0.cbData = 0;
            }
        }
    }
    fn input_blob(bytes: &[u8]) -> Result<CRYPT_INTEGER_BLOB, PersistenceError> {
        Ok(CRYPT_INTEGER_BLOB {
            cbData: u32::try_from(bytes.len()).map_err(|_| PersistenceError::InvalidArchive)?,
            pbData: if bytes.is_empty() {
                std::ptr::null_mut()
            } else {
                bytes.as_ptr().cast_mut()
            },
        })
    }

    let input = input_blob(plaintext)?;
    let entropy = input_blob(ENTROPY)?;
    let mut output = LocalBlob::default();
    unsafe {
        // No CRYPTPROTECT_LOCAL_MACHINE: this ciphertext is bound to the
        // currently signed-in Windows user. UI is forbidden for wallpaper use.
        CryptProtectData(
            &input,
            w!("dsh-wallpaper API conversations"),
            Some(&entropy),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output.0,
        )
    }
    .map_err(|_| PersistenceError::Encryption)?;
    Ok(output.as_bytes().to_vec())
}

#[cfg(windows)]
fn dpapi_unprotect(ciphertext: &[u8]) -> Result<Vec<u8>, PersistenceError> {
    use windows::Win32::{
        Foundation::{LocalFree, HLOCAL},
        Security::Cryptography::{
            CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        },
    };

    const ENTROPY: &[u8] = b"dsh-wallpaper/api-conversations/v1";

    struct LocalBlob(CRYPT_INTEGER_BLOB);
    impl Default for LocalBlob {
        fn default() -> Self {
            Self(CRYPT_INTEGER_BLOB::default())
        }
    }
    impl LocalBlob {
        fn as_bytes(&self) -> &[u8] {
            if self.0.cbData == 0 {
                &[]
            } else {
                debug_assert!(!self.0.pbData.is_null());
                unsafe { std::slice::from_raw_parts(self.0.pbData, self.0.cbData as usize) }
            }
        }
    }
    impl Drop for LocalBlob {
        fn drop(&mut self) {
            if !self.0.pbData.is_null() {
                unsafe {
                    let _ = LocalFree(Some(HLOCAL(self.0.pbData.cast())));
                }
                self.0.pbData = std::ptr::null_mut();
                self.0.cbData = 0;
            }
        }
    }
    fn input_blob(bytes: &[u8]) -> Result<CRYPT_INTEGER_BLOB, PersistenceError> {
        Ok(CRYPT_INTEGER_BLOB {
            cbData: u32::try_from(bytes.len()).map_err(|_| PersistenceError::InvalidArchive)?,
            pbData: if bytes.is_empty() {
                std::ptr::null_mut()
            } else {
                bytes.as_ptr().cast_mut()
            },
        })
    }

    let input = input_blob(ciphertext)?;
    let entropy = input_blob(ENTROPY)?;
    let mut output = LocalBlob::default();
    unsafe {
        // `None` avoids a second DPAPI-allocated description buffer.
        CryptUnprotectData(
            &input,
            None,
            Some(&entropy),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output.0,
        )
    }
    .map_err(|_| PersistenceError::Decryption)?;
    Ok(output.as_bytes().to_vec())
}

#[cfg(not(windows))]
fn dpapi_protect(_: &[u8]) -> Result<Vec<u8>, PersistenceError> {
    Err(PersistenceError::Unavailable)
}

#[cfg(not(windows))]
fn dpapi_unprotect(_: &[u8]) -> Result<Vec<u8>, PersistenceError> {
    Err(PersistenceError::Unavailable)
}

#[cfg(all(test, windows))]
mod tests {
    use super::EncryptedJsonStore;
    use serde::{Deserialize, Serialize};
    use std::fs;

    #[derive(Debug, Deserialize, PartialEq, Serialize)]
    struct Fixture {
        text: String,
        count: u32,
    }

    #[test]
    fn round_trips_without_storing_plaintext() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("api-conversations.v1.dpapi");
        let store = EncryptedJsonStore::new(&path);
        let fixture = Fixture {
            text: "敏感会话正文".into(),
            count: 7,
        };

        store.save(&fixture).expect("save encrypted transcript");
        let bytes = fs::read(&path).expect("ciphertext");
        assert!(!bytes
            .windows("敏感会话正文".len())
            .any(|window| window == "敏感会话正文".as_bytes()));
        assert_eq!(
            store.load::<Fixture>().expect("load transcript"),
            Some(fixture)
        );
    }

    #[test]
    fn corrupt_ciphertext_is_never_replaced_during_load() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("api-conversations.v1.dpapi");
        let store = EncryptedJsonStore::new(&path);
        store
            .save(&Fixture {
                text: "original".into(),
                count: 1,
            })
            .expect("save");
        fs::write(&path, b"not a DPAPI blob").expect("corrupt ciphertext");
        let before = fs::read(&path).expect("before");

        assert!(store.load::<Fixture>().is_err());
        assert_eq!(fs::read(&path).expect("after"), before);
    }
}
