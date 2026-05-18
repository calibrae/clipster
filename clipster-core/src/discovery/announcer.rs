use anyhow::{Context, Result};
use mdns_sd::{ServiceDaemon, ServiceInfo};
use std::collections::HashMap;
use std::net::IpAddr;

use super::SERVICE_TYPE;

/// Announces this device on the LAN via mDNS.
pub struct Announcer {
    daemon: ServiceDaemon,
    fullname: String,
}

impl Announcer {
    /// Announce a clipster peer service.
    /// `instance_name` will typically be the device name (sanitized).
    pub fn start(
        instance_name: &str,
        port: u16,
        device_id: &str,
        device_name: &str,
        version: &str,
        capabilities: &[&str],
    ) -> Result<Self> {
        let daemon = ServiceDaemon::new().context("creating mdns daemon")?;
        let host_ips = local_ipv4_addrs();
        let hostname = format!("{}.local.", sanitize(instance_name));

        let mut props: HashMap<String, String> = HashMap::new();
        props.insert("fp".into(), device_id.to_string());
        props.insert("name".into(), device_name.to_string());
        props.insert("ver".into(), version.to_string());
        props.insert("caps".into(), capabilities.join(","));
        props.insert("proto".into(), super::PROTOCOL_VERSION.to_string());

        let service = ServiceInfo::new(
            SERVICE_TYPE,
            &sanitize(instance_name),
            &hostname,
            host_ips.as_slice(),
            port,
            Some(props),
        )
        .context("building ServiceInfo")?;

        let fullname = service.get_fullname().to_string();
        daemon.register(service).context("registering mdns service")?;
        tracing::info!(service = %fullname, port, "mdns announce");

        Ok(Self { daemon, fullname })
    }

    pub fn fullname(&self) -> &str {
        &self.fullname
    }

    pub fn shutdown(&self) {
        let _ = self.daemon.unregister(&self.fullname);
        let _ = self.daemon.shutdown();
    }
}

impl Drop for Announcer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
        .collect()
}

fn local_ipv4_addrs() -> Vec<IpAddr> {
    // Use mdns_sd's helper if available, else fall back to a simple bind trick.
    if let Ok(ifaces) = if_addrs::get_if_addrs() {
        ifaces
            .into_iter()
            .filter(|i| !i.is_loopback())
            .map(|i| i.ip())
            .filter(|ip| ip.is_ipv4())
            .collect()
    } else {
        vec![]
    }
}
