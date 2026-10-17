//! Photo storage: Cloudflare R2 in production, a local folder in development.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use bytes::Bytes;
use object_store::aws::AmazonS3Builder;
use object_store::local::LocalFileSystem;
use object_store::path::Path;
use object_store::signer::Signer;
use object_store::{ObjectStore, PutPayload};

#[derive(Clone)]
pub struct Storage {
    store: Arc<dyn ObjectStore>,
    /// Set for R2: signs short-lived download links for the review page.
    signer: Option<Arc<dyn Signer>>,
    pub backend: &'static str,
}

impl Storage {
    /// R2 when `R2_ACCOUNT_ID`, `R2_ACCESS_KEY_ID`, `R2_SECRET_ACCESS_KEY` and
    /// `R2_BUCKET` are set; otherwise files go under `LOCAL_STORAGE_DIR` (default `./data`).
    pub fn from_env() -> anyhow::Result<Self> {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        if let (Some(account), Some(key), Some(secret), Some(bucket)) = (
            var("R2_ACCOUNT_ID"),
            var("R2_ACCESS_KEY_ID"),
            var("R2_SECRET_ACCESS_KEY"),
            var("R2_BUCKET"),
        ) {
            let s3 = Arc::new(
                AmazonS3Builder::new()
                    .with_endpoint(format!("https://{account}.r2.cloudflarestorage.com"))
                    .with_region("auto")
                    .with_bucket_name(bucket)
                    .with_access_key_id(key)
                    .with_secret_access_key(secret)
                    .build()
                    .context("configuring R2")?,
            );
            return Ok(Self {
                store: s3.clone(),
                signer: Some(s3),
                backend: "r2",
            });
        }
        let dir = var("LOCAL_STORAGE_DIR").unwrap_or_else(|| "./data".into());
        std::fs::create_dir_all(&dir)?;
        let local = LocalFileSystem::new_with_prefix(&dir)?;
        Ok(Self {
            store: Arc::new(local),
            signer: None,
            backend: "local",
        })
    }

    pub async fn put(&self, key: &str, bytes: Bytes) -> anyhow::Result<()> {
        self.store
            .put(&Path::from(key), PutPayload::from_bytes(bytes))
            .await?;
        Ok(())
    }

    pub async fn get(&self, key: &str) -> anyhow::Result<Bytes> {
        Ok(self.store.get(&Path::from(key)).await?.bytes().await?)
    }

    pub async fn delete(&self, key: &str) -> anyhow::Result<()> {
        self.store.delete(&Path::from(key)).await?;
        Ok(())
    }

    /// A short-lived direct link on R2; `None` locally (serve through the API instead).
    pub async fn signed_url(&self, key: &str, ttl: Duration) -> anyhow::Result<Option<String>> {
        let Some(signer) = &self.signer else {
            return Ok(None);
        };
        let url = signer
            .signed_url(http::Method::GET, &Path::from(key), ttl)
            .await?;
        Ok(Some(url.to_string()))
    }
}
