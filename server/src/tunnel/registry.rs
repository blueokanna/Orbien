use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::sync::Mutex;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunnelOwner {
    pub session_id: String,
    pub generation: u64,
}
#[derive(Debug, Default)]
pub struct TunnelRegistry {
    by_name: Mutex<HashMap<String, TunnelOwner>>,
}
impl TunnelRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn try_insert(&self, name: &str, owner: TunnelOwner) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err(anyhow!("empty tunnel name"));
        }
        let mut map = self.by_name.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = map.get(name) {
            return Err(anyhow!(
                "tunnel `{name}` is already registered (session={}, generation={})",
                existing.session_id,
                existing.generation
            ));
        }
        map.insert(name.to_string(), owner);
        Ok(())
    }

    pub fn remove_if_owner(&self, name: &str, owner: &TunnelOwner) {
        let mut map = self.by_name.lock().unwrap_or_else(|e| e.into_inner());
        if map.get(name) == Some(owner) {
            map.remove(name);
        }
    }
}
