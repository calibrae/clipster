use anyhow::{Context, Result};
use mdns_sd::{ServiceDaemon, ServiceEvent};
use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::sync::broadcast;

use super::SERVICE_TYPE;

#[derive(Debug, Clone)]
pub struct DiscoveredPeer {
    pub device_id: String,
    pub name: String,
    pub addrs: Vec<SocketAddr>,
    pub version: String,
    pub capabilities: Vec<String>,
    pub proto: u32,
    pub fullname: String,
}

#[derive(Debug, Clone)]
pub enum PeerEvent {
    Discovered(DiscoveredPeer),
    Lost { fullname: String },
}

/// Browse the LAN for clipster peers.
pub struct Browser {
    daemon: ServiceDaemon,
    tx: broadcast::Sender<PeerEvent>,
    own_device_id: String,
}

impl Browser {
    pub fn start(own_device_id: String) -> Result<Self> {
        let daemon = ServiceDaemon::new().context("creating mdns daemon")?;
        let receiver = daemon
            .browse(SERVICE_TYPE)
            .context("starting mdns browse")?;
        let (tx, _) = broadcast::channel::<PeerEvent>(64);

        let tx2 = tx.clone();
        let own_id = own_device_id.clone();
        std::thread::spawn(move || {
            while let Ok(event) = receiver.recv() {
                match event {
                    ServiceEvent::ServiceResolved(info) => {
                        let props: HashMap<String, String> = info
                            .get_properties()
                            .iter()
                            .map(|p| (p.key().to_string(), p.val_str().to_string()))
                            .collect();

                        let device_id = match props.get("fp") {
                            Some(v) => v.clone(),
                            None => continue,
                        };

                        // Skip ourselves
                        if device_id == own_id {
                            continue;
                        }

                        let name = props.get("name").cloned().unwrap_or_else(|| info.get_hostname().to_string());
                        let version = props.get("ver").cloned().unwrap_or_default();
                        let caps: Vec<String> = props
                            .get("caps")
                            .map(|s| s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect())
                            .unwrap_or_default();
                        let proto: u32 = props.get("proto").and_then(|s| s.parse().ok()).unwrap_or(1);

                        let port = info.get_port();
                        let addrs: Vec<SocketAddr> = info
                            .get_addresses()
                            .iter()
                            .map(|ip| SocketAddr::new(*ip, port))
                            .collect();

                        if addrs.is_empty() {
                            continue;
                        }

                        let _ = tx2.send(PeerEvent::Discovered(DiscoveredPeer {
                            device_id,
                            name,
                            addrs,
                            version,
                            capabilities: caps,
                            proto,
                            fullname: info.get_fullname().to_string(),
                        }));
                    }
                    ServiceEvent::ServiceRemoved(_ty, fullname) => {
                        let _ = tx2.send(PeerEvent::Lost { fullname });
                    }
                    _ => {}
                }
            }
        });

        Ok(Self {
            daemon,
            tx,
            own_device_id,
        })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<PeerEvent> {
        self.tx.subscribe()
    }

    pub fn own_device_id(&self) -> &str {
        &self.own_device_id
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        let _ = self.daemon.shutdown();
    }
}
