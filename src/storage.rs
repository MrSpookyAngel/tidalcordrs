pub struct LRUStorage {
    pub storage_dir: std::path::PathBuf,
    pub max_size: u64, // bytes
    lock: std::sync::Arc<tokio::sync::Mutex<()>>,
}

impl LRUStorage {
    pub fn new(storage_dir: &str, max_size: u64) -> Self {
        let dir = std::path::PathBuf::from(storage_dir);
        std::fs::create_dir_all(&dir).unwrap();
        LRUStorage {
            storage_dir: dir,
            max_size,
            lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    async fn evict_if_needed(&self) -> Result<(), std::io::Error> {
        let _ = self.lock.lock().await;
        
        let mut entries = tokio::fs::read_dir(&self.storage_dir).await?;
        let mut files: Vec<(std::path::PathBuf, u64, u64)> = Vec::new();
        let mut total_size = 0;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let metadata = entry.metadata().await?;
            if metadata.is_file() {
                let file_size = metadata.len() as u64;
                let age_secs = metadata.accessed().unwrap().elapsed().unwrap().as_secs() as u64;

                total_size += file_size;
                files.push((path, file_size, age_secs));
            }
        }

        if total_size > self.max_size {
            files.sort_by_key(|&(_, _, age)| std::cmp::Reverse(age));

            for (evict_path, file_size, _) in files {
                if total_size <= self.max_size {
                    break;
                }
                total_size -= file_size;
                tokio::fs::remove_file(&evict_path).await?;
                println!("Evicted file: {:?}", evict_path);
            }
        }

        Ok(())
    }

    pub async fn exists(&self, key: &str) -> bool {
        let _ = self.lock.lock().await;

        let path = self.storage_dir.join(key);
        tokio::fs::try_exists(&path).await.ok().unwrap_or(false)
    }

    pub async fn insert(&self, key: String, value: Vec<u8>) -> Result<(), std::io::Error> {
        let _ = self.lock.lock().await;
        
        let path = self.storage_dir.join(key);
        tokio::fs::write(path, value).await?;
        self.evict_if_needed().await?;
        Ok(())
    }
}
