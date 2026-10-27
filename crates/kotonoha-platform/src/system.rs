use std::sync::Mutex;

use kotonoha_core::ports::SystemProbe;
use sysinfo::{MemoryRefreshKind, RefreshKind, System};

/// Free memory as the OS reports it (reclaimable caches included).
pub struct MemoryProbe {
    system: Mutex<System>,
}

impl Default for MemoryProbe {
    fn default() -> Self {
        let refresh = RefreshKind::nothing().with_memory(MemoryRefreshKind::nothing().with_ram());
        Self { system: Mutex::new(System::new_with_specifics(refresh)) }
    }
}

impl SystemProbe for MemoryProbe {
    fn available_memory_mb(&self) -> u64 {
        let mut system = self.system.lock().unwrap();
        system.refresh_memory_specifics(MemoryRefreshKind::nothing().with_ram());
        system.available_memory() / (1024 * 1024)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_some_memory() {
        assert!(MemoryProbe::default().available_memory_mb() > 0);
    }
}
