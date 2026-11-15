//! What kind of network this computer is on.

/// Windows marks networks in cafés, airports and hotels as "Public". A computer on one does not
/// accept work, whatever the firewall allows. Other systems have no such marking: always `false`.
pub fn on_public_network() -> bool {
    #[cfg(windows)]
    {
        windows_impl::on_public_network().unwrap_or_else(|e| {
            tracing::warn!(error = %e, "could not read the network category");
            false
        })
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(windows)]
mod windows_impl {
    use windows::Win32::Networking::NetworkListManager::{
        INetworkListManager, NetworkListManager, NLM_ENUM_NETWORK_CONNECTED, NLM_NETWORK_CATEGORY_PUBLIC,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
    };

    pub fn on_public_network() -> windows::core::Result<bool> {
        // SAFETY: COM is initialised for this call only and released before returning
        unsafe {
            let initialized = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            let result = (|| {
                let manager: INetworkListManager = CoCreateInstance(&NetworkListManager, None, CLSCTX_ALL)?;
                let networks = manager.GetNetworks(NLM_ENUM_NETWORK_CONNECTED)?;
                let mut public = false;
                loop {
                    let mut batch = [None];
                    let mut fetched = 0;
                    networks.Next(&mut batch, Some(&mut fetched))?;
                    if fetched == 0 {
                        break;
                    }
                    if let Some(network) = batch[0].take() {
                        public |= network.GetCategory()? == NLM_NETWORK_CATEGORY_PUBLIC;
                    }
                }
                Ok(public)
            })();
            if initialized {
                CoUninitialize();
            }
            result
        }
    }
}
