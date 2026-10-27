//! Downloads models and tool builds with checksum verification. Data goes to a `.part` file that is
//! renamed only after its SHA-256 matches, so an interrupted or tampered download is never used.

use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use kotonoha_core::models::CatalogModel;
use kotonoha_core::setup::{Destination, Download};
use sha2::{Digest, Sha256};

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("download failed: {0}")]
    Http(#[from] ureq::Error),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("checksum mismatch for {file}")]
    Checksum { file: String },
    #[error("cancelled")]
    Cancelled,
}

/// SHA-256 of a file as lowercase hex.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buf = vec![0; 1 << 20];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex(&hasher.finalize()))
}

/// Whether `dir` holds `model` with the right size and checksum.
pub fn is_installed(model: &CatalogModel, dir: &Path) -> bool {
    let path = dir.join(model.file);
    std::fs::metadata(&path).is_ok_and(|m| m.len() == model.bytes)
        && sha256_file(&path).is_ok_and(|sum| sum == model.sha256)
}

/// Where each kind of download is saved.
pub struct Folders<'a> {
    pub tools: &'a Path,
    pub models: &'a Path,
}

impl Folders<'_> {
    fn of(&self, destination: Destination) -> &Path {
        match destination {
            Destination::Tools => self.tools,
            Destination::Models => self.models,
        }
    }
}

/// Downloads `item` into its folder, calling `progress(done, total)` as bytes arrive. Blocking; stops
/// with [`DownloadError::Cancelled`] soon after `cancel` is set. Tools are made executable.
pub fn fetch(
    item: &Download,
    folders: &Folders<'_>,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64, u64),
) -> Result<PathBuf, DownloadError> {
    let dir = folders.of(item.destination);
    std::fs::create_dir_all(dir)?;
    let target = dir.join(&item.file);
    let part = dir.join(format!("{}.part", item.file));

    let result = (|| {
        let mut response = ureq::get(item.url).call()?;
        let mut reader = response.body_mut().as_reader();
        let mut file = File::create(&part)?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0; 1 << 20];
        let mut done = 0;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(DownloadError::Cancelled);
            }
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])?;
            hasher.update(&buf[..n]);
            done += n as u64;
            progress(done, item.bytes);
        }
        file.sync_all()?;
        if hex(&hasher.finalize()) != item.sha256 {
            return Err(DownloadError::Checksum { file: item.file.clone() });
        }
        Ok(())
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    if item.destination == Destination::Tools {
        make_executable(&part)?;
    }
    std::fs::rename(&part, &target)?;
    Ok(target)
}

/// Downloads a catalog model into `dir`. Blocking.
pub fn download(model: &CatalogModel, dir: &Path, progress: impl FnMut(u64, u64)) -> Result<PathBuf, DownloadError> {
    let folders = Folders { tools: dir, models: dir };
    fetch(&Download::model(model), &folders, &AtomicBool::new(false), progress)
}

#[cfg(unix)]
fn make_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn make_executable(_: &Path) -> std::io::Result<()> {
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use std::io::BufRead;
    use std::net::TcpListener;

    use kotonoha_core::models::{MemoryMode, ModelKind};

    use super::*;

    #[test]
    fn hashes_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a");
        std::fs::write(&path, "abc").unwrap();
        assert_eq!(sha256_file(&path).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    /// Serves `body` once over plain HTTP on a local port and returns its URL.
    fn serve_once(body: &'static [u8]) -> &'static str {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/file", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap() > 2 {
                line.clear();
            }
            let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            stream.write_all(head.as_bytes()).unwrap();
            stream.write_all(body).unwrap();
        });
        Box::leak(url.into_boxed_str())
    }

    fn item(url: &'static str, sha256: &'static str, destination: Destination) -> Download {
        Download { file: "whisper-cli".into(), url, sha256, bytes: 3, destination, purpose: ModelKind::Whisper }
    }

    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    #[test]
    fn verified_tool_lands_in_the_tools_folder_and_is_executable() {
        let root = tempfile::tempdir().unwrap();
        let (tools, models) = (root.path().join("bin"), root.path().join("models"));
        let folders = Folders { tools: &tools, models: &models };
        let mut seen = 0;
        let path =
            fetch(&item(serve_once(b"abc"), ABC, Destination::Tools), &folders, &AtomicBool::new(false), |d, _| {
                seen = d
            })
            .unwrap();
        assert_eq!(path, tools.join("whisper-cli"));
        assert_eq!(std::fs::read(&path).unwrap(), b"abc");
        assert_eq!(seen, 3);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o111, 0o111);
        }
    }

    #[test]
    fn wrong_checksum_leaves_nothing_behind() {
        let root = tempfile::tempdir().unwrap();
        let folders = Folders { tools: root.path(), models: root.path() };
        let wrong = "0".repeat(64).leak();
        let result =
            fetch(&item(serve_once(b"abc"), wrong, Destination::Models), &folders, &AtomicBool::new(false), |_, _| {});
        assert!(matches!(result, Err(DownloadError::Checksum { .. })));
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }

    /// Fetches the smallest published tool build over the network, through GitHub's redirect to its file
    /// storage. Run with `cargo test -p kotonoha-platform -- --ignored` after pinning a new release.
    #[test]
    #[ignore = "network"]
    fn published_tool_build_downloads_and_verifies() {
        use kotonoha_core::setup::{plan, Present};
        use kotonoha_core::tools::{Target, TOOL_CATALOG};

        let root = tempfile::tempdir().unwrap();
        let folders = Folders { tools: root.path(), models: root.path() };
        let planned = plan(MemoryMode::Normal, Some(Target::WindowsX64), Present::default(), TOOL_CATALOG);
        let whisper = planned.downloads.iter().find(|d| d.destination == Destination::Tools).unwrap();
        let path = fetch(whisper, &folders, &AtomicBool::new(false), |_, _| {}).unwrap();
        assert_eq!(path, root.path().join("whisper-cli.exe"));
        assert_eq!(sha256_file(&path).unwrap(), whisper.sha256);
    }

    #[test]
    fn cancelled_download_leaves_nothing_behind() {
        let root = tempfile::tempdir().unwrap();
        let folders = Folders { tools: root.path(), models: root.path() };
        let result =
            fetch(&item(serve_once(b"abc"), ABC, Destination::Models), &folders, &AtomicBool::new(true), |_, _| {});
        assert!(matches!(result, Err(DownloadError::Cancelled)));
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }
}
