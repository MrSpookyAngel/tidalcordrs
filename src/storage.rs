pub struct Storage {
    cache: foyer::HybridCache<String, String>,
}

impl Storage {
    // Persistent LRU cache
    pub async fn new(cache_dir: &str, cache_max_size: usize) -> Result<Self, std::io::Error> {
        let builder = foyer::HybridCacheBuilder::new()
            .memory(cache_max_size)
            .with_eviction_config(foyer::EvictionConfig::Lru(foyer::LruConfig::default()))
            .with_weighter(|_key, path: &String| {
                std::fs::metadata(std::path::Path::new(path))
                    .map(|metadata| metadata.len() as usize)
                    .unwrap_or(0)
            })
            .storage(foyer::Engine::Large)
            .with_device_options(
                foyer::DirectFsDeviceOptions::new(cache_dir).with_capacity(cache_max_size),
            );

        let hybrid = builder.build().await.map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to create hybrid cache: {}", e),
            )
        })?;

        Ok(Self { cache: hybrid })
    }

    pub async fn get(&self, key: &str) -> Result<Option<String>, std::io::Error> {
        let entry = self
            .cache
            .get(&key.to_string())
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        Ok(entry.map(|entry| entry.value().clone()))
    }

    pub fn insert(&self, key: String, value: String) {
        self.cache.insert(key, value);
    }
}
