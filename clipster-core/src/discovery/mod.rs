mod browser;
mod announcer;

pub use announcer::Announcer;
pub use browser::{Browser, PeerEvent, DiscoveredPeer};

pub const SERVICE_TYPE: &str = "_clipster._tcp.local.";
pub const PROTOCOL_VERSION: u32 = 1;
