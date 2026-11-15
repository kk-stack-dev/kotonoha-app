//! This computer's long-term key pair. Paired computers recognise each other by their public keys.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use kotonoha_core::lan::pairing::{fingerprint, from_hex, hex};

use super::transport::NOISE_PARAMS;
use super::LanError;

pub struct Identity {
    private: [u8; 32],
    public: [u8; 32],
}

impl Identity {
    pub fn generate() -> Result<Self, LanError> {
        let params = NOISE_PARAMS.parse().expect("valid Noise parameters");
        let pair = snow::Builder::new(params).generate_keypair()?;
        let key = |v: Vec<u8>| v.try_into().map_err(|_| LanError::Key("unexpected key length".into()));
        Ok(Self { private: key(pair.private)?, public: key(pair.public)? })
    }

    pub(crate) fn private(&self) -> &[u8; 32] {
        &self.private
    }

    pub fn public(&self) -> &[u8; 32] {
        &self.public
    }

    /// The short id announced on the LAN.
    pub fn id(&self) -> String {
        fingerprint(&self.public)
    }

    /// Reads the key file, or creates it (readable only by this user) when it does not exist yet.
    pub fn load_or_create(path: &Path) -> Result<Self, LanError> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                Self::parse(&text).ok_or_else(|| LanError::Key(format!("{} is not a key file", path.display())))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let identity = Self::generate()?;
                identity.save(path)?;
                tracing::info!(id = %identity.id(), "created this computer's LAN key");
                Ok(identity)
            }
            Err(e) => Err(LanError::Key(e.to_string())),
        }
    }

    fn parse(text: &str) -> Option<Self> {
        let mut lines = text.lines();
        let private = from_hex::<32>(lines.next()?.trim())?;
        let public = from_hex::<32>(lines.next()?.trim())?;
        Some(Self { private, public })
    }

    fn save(&self, path: &Path) -> Result<(), LanError> {
        let err = |e: std::io::Error| LanError::Key(e.to_string());
        let dir = path.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(dir).map_err(err)?;
        let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(err)?;
        // tempfile already creates the file as 0600 on Unix; Windows keeps it in the user's profile
        writeln!(tmp, "{}\n{}", hex(&self.private), hex(&self.public)).map_err(err)?;
        tmp.persist(path).map_err(|e| err(e.error))?;
        Ok(())
    }
}

/// Loads the key on first use, so no key file exists until a LAN feature is turned on.
pub struct IdentitySource {
    path: PathBuf,
    loaded: Mutex<Option<Arc<Identity>>>,
}

impl IdentitySource {
    pub fn new(path: PathBuf) -> Self {
        Self { path, loaded: Mutex::new(None) }
    }

    pub fn get(&self) -> Result<Arc<Identity>, LanError> {
        let mut loaded = self.loaded.lock().unwrap();
        if let Some(identity) = &*loaded {
            return Ok(identity.clone());
        }
        let identity = Arc::new(Identity::load_or_create(&self.path)?);
        *loaded = Some(identity.clone());
        Ok(identity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn created_once_then_reloaded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config").join("lan-key");
        let first = Identity::load_or_create(&path).unwrap();
        let again = Identity::load_or_create(&path).unwrap();
        assert_eq!(first.public(), again.public());
        assert_eq!(first.private(), again.private());
        assert_ne!(first.public(), Identity::generate().unwrap().public());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        std::fs::write(&path, "nonsense").unwrap();
        assert!(matches!(Identity::load_or_create(&path), Err(LanError::Key(_))));
    }

    #[test]
    fn source_creates_nothing_until_asked() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lan-key");
        let source = IdentitySource::new(path.clone());
        assert!(!path.exists());
        let id = source.get().unwrap().id();
        assert!(path.exists());
        assert_eq!(source.get().unwrap().id(), id);
    }
}
