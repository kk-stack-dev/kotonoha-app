//! Paired computers, kept in their own file so saving settings from the UI never overwrites them.

use std::path::PathBuf;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

use crate::store::{read_json, write_json, StoreError};

/// The computer this one sends work to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedServer {
    /// Public key, hex.
    pub key: String,
    /// The short id it announces on the LAN.
    pub id: String,
    pub name: String,
    /// Where it was last reached ("192.168.1.20:50540"); found again with mDNS when it moves.
    pub address: Option<String>,
}

/// A computer allowed to send work to this one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedClient {
    pub key: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Peers {
    pub server: Option<PairedServer>,
    pub clients: Vec<PairedClient>,
}

pub struct PeerStore {
    path: PathBuf,
    peers: RwLock<Peers>,
}

impl PeerStore {
    pub fn open(path: PathBuf) -> Self {
        let peers = read_json(&path).unwrap_or_default();
        Self { path, peers: RwLock::new(peers) }
    }

    pub fn get(&self) -> Peers {
        self.peers.read().unwrap().clone()
    }

    pub fn update(&self, change: impl FnOnce(&mut Peers)) -> Result<(), StoreError> {
        let mut peers = self.peers.write().unwrap();
        let mut next = peers.clone();
        change(&mut next);
        if next != *peers {
            write_json(&self.path, &next)?;
            *peers = next;
        }
        Ok(())
    }

    pub fn is_client(&self, key_hex: &str) -> bool {
        self.peers.read().unwrap().clients.iter().any(|c| c.key == key_hex)
    }

    /// Adds or renames a client.
    pub fn add_client(&self, client: PairedClient) -> Result<(), StoreError> {
        self.update(|p| {
            p.clients.retain(|c| c.key != client.key);
            p.clients.push(client);
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clients_are_added_once_and_saved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lan-peers.json");
        let store = PeerStore::open(path.clone());
        assert_eq!(store.get(), Peers::default());
        store.add_client(PairedClient { key: "aa".into(), name: "A".into() }).unwrap();
        store.add_client(PairedClient { key: "aa".into(), name: "A2".into() }).unwrap();
        assert!(store.is_client("aa"));
        assert!(!store.is_client("bb"));
        let reopened = PeerStore::open(path);
        assert_eq!(reopened.get().clients, [PairedClient { key: "aa".into(), name: "A2".into() }]);
    }
}
