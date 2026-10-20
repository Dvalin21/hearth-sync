use crate::error::{AppError, Result};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct ObjectStore {
    root: PathBuf,
    thumbs: PathBuf,
}

#[derive(Clone, Debug)]
pub struct StoredObject {
    pub hash: String,
    pub size: u64,
    pub path: PathBuf,
}

impl ObjectStore {
    pub fn new(root: PathBuf, thumbs: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&root)?;
        std::fs::create_dir_all(&thumbs)?;
        Ok(Self { root, thumbs })
    }

    pub fn hash_bytes(data: &[u8]) -> String {
        let mut h = Sha256::new();
        h.update(data);
        hex::encode(h.finalize())
    }

    pub fn object_path(&self, hash: &str) -> PathBuf {
        // 2-char prefix sharding
        let prefix = &hash[..2.min(hash.len())];
        self.root.join(prefix).join(hash)
    }

    pub fn thumb_path(&self, hash: &str) -> PathBuf {
        self.thumbs.join(format!("{}.jpg", hash))
    }

    /// Store bytes if not already present. Returns hash + size.
    pub fn put(&self, data: &[u8]) -> Result<StoredObject> {
        let hash = Self::hash_bytes(data);
        let path = self.object_path(&hash);
        if !path.exists() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, data)?;
        }
        Ok(StoredObject {
            hash,
            size: data.len() as u64,
            path,
        })
    }

    pub fn get(&self, hash: &str) -> Result<Vec<u8>> {
        let path = self.object_path(hash);
        if !path.exists() {
            return Err(AppError::NotFound);
        }
        Ok(std::fs::read(path)?)
    }

    pub fn delete_if_unreferenced(&self, hash: &str, refcount: i64) -> Result<()> {
        if refcount <= 0 {
            let path = self.object_path(hash);
            let _ = std::fs::remove_file(path);
            let tp = self.thumb_path(hash);
            let _ = std::fs::remove_file(tp);
        }
        Ok(())
    }

    pub fn thumbs_dir(&self) -> &Path {
        &self.thumbs
    }
}
