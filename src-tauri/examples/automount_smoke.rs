//! Verbose manual smoke test for the auto-mount D-Bus path.
//!
//! Support / development tool — not part of the production binary.
//! Linux only (the automount module compiles to a no-op on macOS /
//! Windows, and `zbus` is gated to Linux in `Cargo.toml`). Run on a
//! Linux host where a Lunii is plugged in to see every block device
//! udisks2 exposes, the corresponding `Drive` / `IdType` / `IdUsage`
//! properties, and the final per-device automount outcome. Useful to
//! diagnose "I plugged in my Lunii but Rustory does not see it"
//! reports.
//!
//! Usage: `cargo run --example automount_smoke`

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("automount_smoke is Linux-only — no-op on this platform.");
}

#[cfg(target_os = "linux")]
use std::collections::HashMap;

#[cfg(target_os = "linux")]
use zbus::blocking::{Connection, Proxy};
#[cfg(target_os = "linux")]
use zbus::names::InterfaceName;
#[cfg(target_os = "linux")]
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

const UDISKS2_BUS: &str = "org.freedesktop.UDisks2";
const MANAGER_PATH: &str = "/org/freedesktop/UDisks2/Manager";
const MANAGER_IFACE: &str = "org.freedesktop.UDisks2.Manager";
const PROPS_IFACE: &str = "org.freedesktop.DBus.Properties";
const BLOCK_IFACE: &str = "org.freedesktop.UDisks2.Block";

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Rustory automount smoke (verbose) ===");

    println!("Step 1: open system D-Bus connection ...");
    let conn = Connection::system()?;
    println!("  OK");

    println!("Step 2: call Manager.GetBlockDevices ...");
    let proxy = Proxy::new(
        &conn,
        UDISKS2_BUS,
        MANAGER_PATH,
        InterfaceName::try_from(MANAGER_IFACE)?,
    )?;
    let options: HashMap<String, OwnedValue> = HashMap::new();
    let paths: Vec<OwnedObjectPath> = proxy.call("GetBlockDevices", &(options,))?;
    println!("  Got {} block device paths", paths.len());

    for (i, path) in paths.iter().enumerate() {
        println!("\n[Block {i}] {}", path.as_str());
        let props_proxy = Proxy::new(
            &conn,
            UDISKS2_BUS,
            path.as_str(),
            InterfaceName::try_from(PROPS_IFACE)?,
        )?;
        let props: HashMap<String, OwnedValue> = match props_proxy.call("GetAll", &(BLOCK_IFACE,)) {
            Ok(p) => p,
            Err(e) => {
                println!("  GetAll(Block) error: {e}");
                continue;
            }
        };
        for k in ["Drive", "IdType", "IdUsage", "Device", "IdLabel"] {
            if let Some(v) = props.get(k) {
                println!("  {k}: {v:?}");
            }
        }
    }

    println!("\nStep 3: call rustory_lib::try_automount ...");
    let attempts = rustory_lib::infrastructure::device::try_automount_lunii_candidates();
    println!("  Attempts: {}", attempts.len());
    for (i, a) in attempts.iter().enumerate() {
        println!("  [{i}] device={} outcome={:?}", a.device, a.outcome);
    }
    Ok(())
}
