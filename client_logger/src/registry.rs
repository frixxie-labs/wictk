use std::collections::BTreeMap;

use anyhow::{anyhow, Result};
use reqwest::{Client, Url};
use serde::{de::DeserializeOwned, Serialize};

use crate::api::{get_json, post_json};

/// An entity stored in a hemrs collection that is looked up by a natural key.
pub trait Registered: DeserializeOwned {
    type Key: Ord + Clone + std::fmt::Debug;
    type Id: Copy;
    /// The payload used to create the entity; it has no id yet.
    type New: Serialize;

    fn key(&self) -> Self::Key;
    fn id(&self) -> Self::Id;
    fn new_key(new: &Self::New) -> Self::Key;
}

/// Finds entities by key in a hemrs collection, creating them when missing.
pub struct Registry<T: Registered> {
    client: Client,
    url: Url,
    known: BTreeMap<T::Key, T::Id>,
}

impl<T: Registered> Registry<T> {
    pub fn new(client: Client, url: Url) -> Self {
        Self {
            client,
            url,
            known: BTreeMap::new(),
        }
    }

    pub fn cached(&self, key: &T::Key) -> Option<T::Id> {
        self.known.get(key).copied()
    }

    pub async fn refresh(&mut self) -> Result<()> {
        let entries: Vec<T> = get_json(&self.client, self.url.clone(), &[]).await?;
        self.known
            .extend(entries.iter().map(|entry| (entry.key(), entry.id())));
        Ok(())
    }

    /// Returns the id for `new`'s key, creating the entity if it does not exist.
    pub async fn ensure(&mut self, new: &T::New) -> Result<T::Id> {
        let key = T::new_key(new);
        if let Some(id) = self.cached(&key) {
            return Ok(id);
        }
        self.refresh().await?;
        if let Some(id) = self.cached(&key) {
            return Ok(id);
        }

        tracing::info!("Creating {key:?}");
        post_json(&self.client, self.url.clone(), new).await?;
        self.refresh().await?;
        self.cached(&key)
            .ok_or_else(|| anyhow!("{key:?} not found after creation"))
    }
}
