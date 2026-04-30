// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Loopback IP address manager for use in tests.
//!
//! Provides [`LoopbackIpManager`] for managing temporary IP addresses on a
//! loopback interface with cross-process reference counting, ensuring proper
//! cleanup even if tests panic.

use network_interface::{NetworkInterface, NetworkInterfaceConfig};
use slog::{error, info, Logger};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::IpAddr;
use std::os::unix::io::AsRawFd;
use std::process::Command;
use std::sync::{Arc, Mutex};

/// Cross-platform file locking using libc's flock(2).
trait FileLockExt {
    fn lock_exclusive(&self) -> std::io::Result<()>;
}

impl FileLockExt for File {
    fn lock_exclusive(&self) -> std::io::Result<()> {
        let fd = self.as_raw_fd();
        let ret = unsafe { libc::flock(fd, libc::LOCK_EX) };
        if ret == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
}

struct ManagedIp {
    address: IpAddr,
    /// Number of [`IpAllocation`]s within this process currently using this IP.
    /// The system-level install/uninstall only happens when this transitions
    /// between 0 and 1. `use_count > 0` implies the IP is installed.
    use_count: u32,
    lockfile: Option<File>,
}

/// RAII guard that ensures proper cleanup of allocated IP addresses when
/// dropped, even if the test panics.
pub struct IpAllocation {
    pub addresses: Vec<IpAddr>,
    pub manager: Arc<Mutex<LoopbackIpManager>>,
}

impl Drop for IpAllocation {
    fn drop(&mut self) {
        let mut manager = self.manager.lock().expect("lock mutex");
        manager.uninstall_addresses(&self.addresses);
    }
}

/// Manages temporary IP addresses on a loopback interface.
///
/// Supports multiple concurrent allocations both within a single process
/// (via in-process reference counting) and across processes (via lockfile-based
/// reference counting in `/tmp`). Addresses are only added to or removed from
/// the system when the reference count transitions between 0 and 1.
pub struct LoopbackIpManager {
    ips: Vec<ManagedIp>,
    ifname: String,
    log: Logger,
}

impl LoopbackIpManager {
    pub fn new(ifname: &str, log: Logger) -> Self {
        Self {
            ips: Vec::new(),
            ifname: ifname.to_string(),
            log,
        }
    }

    fn add(&mut self, addresses: &[IpAddr]) {
        for addr in addresses {
            if !self.ips.iter().any(|ip| ip.address == *addr) {
                self.ips.push(ManagedIp {
                    address: *addr,
                    use_count: 0,
                    lockfile: None,
                });
            }
        }
    }

    /// Allocate IP addresses and return a guard that will clean them up on drop.
    pub fn allocate(
        manager: Arc<Mutex<Self>>,
        addresses: &[IpAddr],
    ) -> Result<IpAllocation, std::io::Error> {
        {
            let mut mgr = manager.lock().expect("lock mutex");
            mgr.add(addresses);
            if let Err(e) = mgr.install(addresses) {
                mgr.uninstall_addresses(addresses);
                return Err(e);
            }
        }
        Ok(IpAllocation {
            addresses: addresses.to_vec(),
            manager,
        })
    }

    fn install(&mut self, addresses: &[IpAddr]) -> Result<(), std::io::Error> {
        let ifname = self.ifname.clone();
        let log = self.log.clone();
        for ip in &mut self.ips {
            if addresses.contains(&ip.address) {
                Self::install_single_ip_static(&ifname, &log, ip)?;
            }
        }
        Ok(())
    }

    /// Skips 127.0.0.1/::1 as they're always present on loopback interfaces.
    fn install_single_ip_static(
        ifname: &str,
        log: &Logger,
        ip: &mut ManagedIp,
    ) -> Result<(), std::io::Error> {
        if is_always_present(ip.address) {
            info!(log, "skipping {} (always present on loopback)", ip.address);
            ip.use_count += 1;
            return Ok(());
        }

        // If already installed by another allocation in this process, nothing
        // more to do — the system IP and lockfile refcount are already set up.
        if ip.use_count > 0 {
            ip.use_count += 1;
            info!(
                log,
                "{}: already installed, use_count now {}",
                ip.address,
                ip.use_count,
            );
            return Ok(());
        }

        // First use in this process: acquire the cross-process lockfile and
        // install the IP if no other process has done so yet.
        let lockfile_path = format!("/tmp/maghemite-ip-{}.lock", ip.address);
        let mut lockfile = flock(&lockfile_path)?;
        let refcount = read_refcount(&mut lockfile);
        if refcount == 0 {
            Self::add_ip_to_system(ifname, log, ip)?;
        }
        let new_refcount = refcount + 1;
        info!(
            log,
            "{}: increment refcount {refcount}->{new_refcount}", ip.address,
        );
        write_refcount(&mut lockfile, new_refcount)?;
        ip.use_count += 1;
        ip.lockfile = Some(lockfile);
        Ok(())
    }

    fn add_ip_to_system(
        ifname: &str,
        log: &Logger,
        ip: &ManagedIp,
    ) -> Result<(), std::io::Error> {
        // Another process may have installed the address while we held the
        // flock.  Skip the shell command to avoid a spurious error.
        if is_addr_on_system(ip.address) {
            info!(log, "{}: already on system, skipping install", ip.address);
            return Ok(());
        }

        let mask = match ip.address {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        };
        let addr_str = format!("{}/{mask}", ip.address);

        #[cfg(target_os = "illumos")]
        let output = {
            let v = match ip.address {
                IpAddr::V4(_) => "v4",
                IpAddr::V6(_) => "v6",
            };
            let mut ip_descr = format!("{v}{}", ip.address);
            ip_descr.retain(|c| c.is_alphanumeric());
            let addr_obj = format!("{}/{}", ifname, ip_descr);
            let cmd = [
                "ipadm",
                "create-addr",
                "-t",
                "-T",
                "static",
                "-a",
                &addr_str,
                &addr_obj,
            ];
            info!(log, "running cmd '{cmd:?}'");
            Command::new("pfexec").args(cmd).output()?
        };

        #[cfg(target_os = "linux")]
        let output = Command::new("sudo")
            .args(["ip", "addr", "add", &addr_str, "dev", ifname])
            .output()?;

        #[cfg(target_os = "macos")]
        let output = {
            let af = match ip.address {
                IpAddr::V4(_) => "inet",
                IpAddr::V6(_) => "inet6",
            };
            Command::new("sudo")
                .args(["ifconfig", ifname, af, &addr_str, "alias"])
                .output()?
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            error!(log, "failed to install {}: {stderr}", ip.address);
            return Err(std::io::Error::other(format!(
                "failed to install {}",
                ip.address
            )));
        }
        info!(log, "added {} to system", ip.address);
        Ok(())
    }

    /// Uninstall specific addresses (called by [`IpAllocation`] on drop).
    pub fn uninstall_addresses(&mut self, addresses: &[IpAddr]) {
        for addr in addresses {
            self.uninstall_single_ip(*addr);
        }
    }

    /// Skips 127.0.0.1/::1 as they should always remain on loopback interfaces.
    fn uninstall_single_ip(&mut self, target_addr: IpAddr) {
        if is_always_present(target_addr) {
            info!(
                self.log,
                "skipping {target_addr} cleanup (always present on loopback)"
            );
            for ip in &mut self.ips {
                if ip.address == target_addr {
                    ip.use_count = ip.use_count.saturating_sub(1);
                    break;
                }
            }
            return;
        }

        for ip in &mut self.ips {
            if ip.address == target_addr && ip.use_count > 0 {
                ip.use_count -= 1;

                if ip.use_count > 0 {
                    info!(
                        self.log,
                        "{}: use_count now {}, keeping installed",
                        ip.address,
                        ip.use_count,
                    );
                    break;
                }

                // Last in-process user: decrement the cross-process refcount
                // and remove the IP from the system if no other process needs it.
                if let Some(mut lockfile) = ip.lockfile.take() {
                    let lockfile_path =
                        format!("/tmp/maghemite-ip-{}.lock", ip.address);
                    let refcount = read_refcount(&mut lockfile);
                    let new_refcount = refcount.saturating_sub(1);
                    info!(
                        self.log,
                        "{}: decrement refcount {refcount}->{new_refcount}",
                        ip.address,
                    );

                    if new_refcount == 0 {
                        Self::remove_ip_from_system_static(
                            &self.ifname,
                            &self.log,
                            ip.address,
                        );
                        drop(lockfile);
                        if let Err(e) = std::fs::remove_file(&lockfile_path) {
                            error!(
                                self.log,
                                "failed to remove lockfile {}: {e}",
                                lockfile_path
                            );
                        } else {
                            info!(
                                self.log,
                                "removed lockfile {}", lockfile_path
                            );
                        }
                    } else {
                        let _ = write_refcount(&mut lockfile, new_refcount);
                    }
                }

                info!(self.log, "uninstalled {}", ip.address);
                break;
            }
        }
    }

    fn remove_ip_from_system_static(ifname: &str, log: &Logger, addr: IpAddr) {
        // Skip the shell command if the address isn't on the system at all.
        if !is_addr_on_system(addr) {
            info!(log, "{addr}: not on system, skipping removal");
            return;
        }

        #[cfg(target_os = "illumos")]
        let output = {
            let v = match addr {
                IpAddr::V4(_) => "v4",
                IpAddr::V6(_) => "v6",
            };
            let mut ip_descr = format!("{v}{addr}");
            ip_descr.retain(|c| c.is_alphanumeric());
            let addr_obj = format!("{ifname}/{ip_descr}");
            Command::new("pfexec")
                .args(["ipadm", "delete-addr", &addr_obj])
                .output()
        };

        #[cfg(target_os = "linux")]
        let output = {
            let mask = match addr {
                IpAddr::V4(_) => 32,
                IpAddr::V6(_) => 128,
            };
            let addr_str = format!("{addr}/{mask}");
            Command::new("sudo")
                .args(["ip", "addr", "del", &addr_str, "dev", ifname])
                .output()
        };

        #[cfg(target_os = "macos")]
        let output = {
            let af = match addr {
                IpAddr::V4(_) => "inet",
                IpAddr::V6(_) => "inet6",
            };
            Command::new("sudo")
                .args(["ifconfig", ifname, af, &addr.to_string(), "-alias"])
                .output()
        };

        match output {
            Ok(output) => {
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    error!(log, "failed to remove {addr} from system: {stderr}");
                    return;
                }
                info!(log, "removed {addr} from system");
            }
            Err(e) => {
                error!(log, "failed to execute remove command for {addr}: {e}");
            }
        }
    }
}

/// Returns true if the address is currently present on any interface.
fn is_addr_on_system(addr: IpAddr) -> bool {
    NetworkInterface::show()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|iface| iface.addr)
        .any(|a| a.ip() == addr)
}

/// Returns true for addresses that are always present on loopback interfaces
/// and should never be installed or removed by the manager.
fn is_always_present(addr: IpAddr) -> bool {
    addr == IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
        || addr == IpAddr::V6(std::net::Ipv6Addr::LOCALHOST)
}

fn flock(path: &str) -> std::io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    file.lock_exclusive()?;
    Ok(file)
}

fn read_refcount(file: &mut File) -> u32 {
    let mut contents = String::new();
    file.seek(SeekFrom::Start(0)).ok();
    file.read_to_string(&mut contents).ok();
    contents.trim().parse().unwrap_or(0)
}

fn write_refcount(file: &mut File, count: u32) -> std::io::Result<()> {
    file.seek(SeekFrom::Start(0))?;
    file.set_len(0)?;
    write!(file, "{}", count)?;
    file.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};

    fn nop_logger() -> slog::Logger {
        slog::Logger::root(slog::Discard, slog::o!())
    }

    /// Returns a unique path in /tmp safe for use across parallel test threads.
    fn temp_path() -> String {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        format!(
            "/tmp/loopback-ip-mgr-test-{}-{}.lock",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        )
    }

    // ── is_always_present ─────────────────────────────────────────────────────

    #[test]
    fn always_present_ipv4_localhost() {
        assert!(is_always_present(IpAddr::V4(Ipv4Addr::LOCALHOST)));
    }

    #[test]
    fn always_present_ipv6_localhost() {
        assert!(is_always_present(IpAddr::V6(Ipv6Addr::LOCALHOST)));
    }

    #[test]
    fn not_always_present_other_ipv4() {
        assert!(!is_always_present(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2))));
        assert!(!is_always_present(IpAddr::V4(Ipv4Addr::new(
            192, 168, 1, 1
        ))));
    }

    #[test]
    fn not_always_present_other_ipv6() {
        assert!(!is_always_present("::2".parse().unwrap()));
        assert!(!is_always_present("fe80::1".parse().unwrap()));
    }

    // ── read_refcount / write_refcount ────────────────────────────────────────

    #[test]
    fn read_refcount_empty_file_returns_zero() {
        let path = temp_path();
        let mut file = File::create(&path).unwrap();
        assert_eq!(read_refcount(&mut file), 0);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn write_then_read_refcount() {
        let path = temp_path();
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .unwrap();
        write_refcount(&mut file, 42).unwrap();
        assert_eq!(read_refcount(&mut file), 42);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn overwrite_refcount() {
        let path = temp_path();
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .unwrap();
        write_refcount(&mut file, 10).unwrap();
        write_refcount(&mut file, 3).unwrap();
        assert_eq!(read_refcount(&mut file), 3);
        std::fs::remove_file(&path).ok();
    }

    // ── flock ─────────────────────────────────────────────────────────────────

    #[test]
    fn flock_creates_and_locks_file() {
        let path = temp_path();
        {
            let _f = flock(&path).expect("flock should succeed");
            assert!(std::path::Path::new(&path).exists());
        }
        std::fs::remove_file(&path).ok();
    }

    // ── LoopbackIpManager – always-present addresses ──────────────────────────
    //
    // 127.0.0.1 and ::1 skip system command execution entirely, so these tests
    // run without elevated privileges.

    #[test]
    fn allocate_loopback_v4() {
        let mgr =
            Arc::new(Mutex::new(LoopbackIpManager::new("lo0", nop_logger())));
        let addr = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let alloc = LoopbackIpManager::allocate(Arc::clone(&mgr), &[addr])
            .expect("allocate loopback v4");
        assert_eq!(alloc.addresses, vec![addr]);
        let inner = mgr.lock().unwrap();
        let ip = inner.ips.iter().find(|ip| ip.address == addr).unwrap();
        assert_eq!(ip.use_count, 1);
    }

    #[test]
    fn allocate_loopback_v6() {
        let mgr =
            Arc::new(Mutex::new(LoopbackIpManager::new("lo0", nop_logger())));
        let addr = IpAddr::V6(Ipv6Addr::LOCALHOST);
        let alloc = LoopbackIpManager::allocate(Arc::clone(&mgr), &[addr])
            .expect("allocate loopback v6");
        assert_eq!(alloc.addresses, vec![addr]);
        let inner = mgr.lock().unwrap();
        let ip = inner.ips.iter().find(|ip| ip.address == addr).unwrap();
        assert_eq!(ip.use_count, 1);
    }

    #[test]
    fn drop_allocation_resets_use_count() {
        let mgr =
            Arc::new(Mutex::new(LoopbackIpManager::new("lo0", nop_logger())));
        let addr = IpAddr::V4(Ipv4Addr::LOCALHOST);
        {
            let _alloc =
                LoopbackIpManager::allocate(Arc::clone(&mgr), &[addr]).unwrap();
            assert_eq!(
                mgr.lock()
                    .unwrap()
                    .ips
                    .iter()
                    .find(|ip| ip.address == addr)
                    .unwrap()
                    .use_count,
                1
            );
        }
        // After drop, use_count should be back to 0.
        assert_eq!(
            mgr.lock()
                .unwrap()
                .ips
                .iter()
                .find(|ip| ip.address == addr)
                .unwrap()
                .use_count,
            0
        );
    }

    #[test]
    fn double_allocate_increments_use_count() {
        let mgr =
            Arc::new(Mutex::new(LoopbackIpManager::new("lo0", nop_logger())));
        let addr = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let _a1 =
            LoopbackIpManager::allocate(Arc::clone(&mgr), &[addr]).unwrap();
        let _a2 =
            LoopbackIpManager::allocate(Arc::clone(&mgr), &[addr]).unwrap();
        assert_eq!(
            mgr.lock()
                .unwrap()
                .ips
                .iter()
                .find(|ip| ip.address == addr)
                .unwrap()
                .use_count,
            2
        );
    }

    #[test]
    fn use_count_decrements_on_partial_drop() {
        let mgr =
            Arc::new(Mutex::new(LoopbackIpManager::new("lo0", nop_logger())));
        let addr = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let _a1 =
            LoopbackIpManager::allocate(Arc::clone(&mgr), &[addr]).unwrap();
        {
            let _a2 =
                LoopbackIpManager::allocate(Arc::clone(&mgr), &[addr]).unwrap();
            assert_eq!(
                mgr.lock()
                    .unwrap()
                    .ips
                    .iter()
                    .find(|ip| ip.address == addr)
                    .unwrap()
                    .use_count,
                2
            );
        }
        // a2 dropped: back to 1.
        assert_eq!(
            mgr.lock()
                .unwrap()
                .ips
                .iter()
                .find(|ip| ip.address == addr)
                .unwrap()
                .use_count,
            1
        );
    }

    #[test]
    fn allocate_multiple_addresses() {
        let mgr =
            Arc::new(Mutex::new(LoopbackIpManager::new("lo0", nop_logger())));
        let addrs = vec![
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
        ];
        let alloc =
            LoopbackIpManager::allocate(Arc::clone(&mgr), &addrs).unwrap();
        assert_eq!(alloc.addresses.len(), 2);
        let inner = mgr.lock().unwrap();
        for addr in &addrs {
            let ip = inner.ips.iter().find(|ip| ip.address == *addr).unwrap();
            assert_eq!(ip.use_count, 1);
        }
    }

    #[test]
    fn add_does_not_duplicate_entries() {
        let mgr =
            Arc::new(Mutex::new(LoopbackIpManager::new("lo0", nop_logger())));
        let addr = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let _a1 =
            LoopbackIpManager::allocate(Arc::clone(&mgr), &[addr]).unwrap();
        let _a2 =
            LoopbackIpManager::allocate(Arc::clone(&mgr), &[addr]).unwrap();
        let inner = mgr.lock().unwrap();
        assert_eq!(inner.ips.iter().filter(|ip| ip.address == addr).count(), 1);
    }

    // ── System install / uninstall (illumos, requires pfexec) ─────────────────
    //
    // These tests actually add/remove addresses on lo0 using ipadm. They rely
    // on the test runner being pfexec (see .cargo/config.toml).
    //
    // Test IPs are from 127.42.0.0/16, which is unused loopback space.

    fn is_addr_installed(addr: IpAddr) -> bool {
        is_addr_on_system(addr)
    }

    #[test]
    #[cfg(target_os = "illumos")]
    fn install_and_uninstall_ipv4() {
        let addr: IpAddr = "127.42.1.1".parse().unwrap();
        assert!(
            !is_addr_installed(addr),
            "127.42.1.1 already present; clean it up first"
        );

        let mgr =
            Arc::new(Mutex::new(LoopbackIpManager::new("lo0", nop_logger())));
        {
            let _alloc = LoopbackIpManager::allocate(Arc::clone(&mgr), &[addr])
                .expect("allocate");
            assert!(is_addr_installed(addr), "address should be installed");
        }
        assert!(
            !is_addr_installed(addr),
            "address should be removed after drop"
        );
    }

    /// Two allocations from the same manager: the IP is installed once and
    /// removed only after both allocations are dropped.
    #[test]
    #[cfg(target_os = "illumos")]
    fn double_alloc_installs_once_removes_once() {
        let addr: IpAddr = "127.42.1.2".parse().unwrap();
        assert!(
            !is_addr_installed(addr),
            "127.42.1.2 already present; clean it up first"
        );

        let mgr =
            Arc::new(Mutex::new(LoopbackIpManager::new("lo0", nop_logger())));
        let a1 =
            LoopbackIpManager::allocate(Arc::clone(&mgr), &[addr]).unwrap();
        let a2 =
            LoopbackIpManager::allocate(Arc::clone(&mgr), &[addr]).unwrap();
        assert!(is_addr_installed(addr));

        drop(a2);
        assert!(
            is_addr_installed(addr),
            "should still be installed while a1 is live"
        );

        drop(a1);
        assert!(
            !is_addr_installed(addr),
            "should be removed after all allocations drop"
        );
    }

    /// Cross-process refcount: a child process acquires the same IP, then exits.
    /// The IP must persist while the parent still holds it and be removed only
    /// after the parent drops its allocation too.
    ///
    /// Coordination uses two signal files:
    /// - READY: child creates this once it has acquired the IP
    /// - RELEASE: parent creates this to tell the child to drop and exit
    ///
    /// The child is launched by re-running the test binary with the helper test
    /// `helper_cross_process_child` selected via an env-var guard.
    #[test]
    #[cfg(target_os = "illumos")]
    fn cross_process_refcount() {
        const ADDR: &str = "127.42.1.3";
        let ready =
            format!("/tmp/loopback-mgr-test-xp-ready-{}", std::process::id());
        let release =
            format!("/tmp/loopback-mgr-test-xp-release-{}", std::process::id());

        let addr: IpAddr = ADDR.parse().unwrap();
        assert!(
            !is_addr_installed(addr),
            "127.42.1.3 already present; clean it up first"
        );

        let _ = std::fs::remove_file(&ready);
        let _ = std::fs::remove_file(&release);

        // Spawn a child process that acquires the IP then waits for our signal.
        // Re-running via pfexec ensures the child has the same privileges.
        let mut child = Command::new("pfexec")
            .arg(std::env::current_exe().unwrap())
            .env("LOOPBACK_HELPER_ADDR", ADDR)
            .env("LOOPBACK_HELPER_READY", &ready)
            .env("LOOPBACK_HELPER_RELEASE", &release)
            .args(["helper_cross_process_child", "--nocapture"])
            .spawn()
            .expect("spawn helper");

        // Wait for child to signal readiness (IP installed, flock held).
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if std::path::Path::new(&ready).exists() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "helper did not signal ready in time"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(
            is_addr_installed(addr),
            "child should have installed the IP"
        );

        // Tell the child to release and wait for it to exit.
        // After the child exits its allocation is dropped (refcount 1 → 0), but
        // because our allocation below also holds a reference the IP should
        // remain installed.
        //
        // NOTE: we acquire the parent allocation AFTER the child exits so that
        // the two exclusive flocks are never held concurrently (the library
        // holds flock(LOCK_EX) for the allocation's entire lifetime, so two
        // concurrent holders in the same kernel lock domain would deadlock).
        File::create(&release).unwrap();
        let status = child.wait().expect("wait for child");
        assert!(status.success(), "helper exited with non-zero status");

        // Child removed the IP (its was the sole holder; refcount 1 → 0).
        // Now the parent re-installs it.
        let mgr =
            Arc::new(Mutex::new(LoopbackIpManager::new("lo0", nop_logger())));
        let alloc =
            LoopbackIpManager::allocate(Arc::clone(&mgr), &[addr]).unwrap();
        assert!(is_addr_installed(addr));

        drop(alloc);
        assert!(
            !is_addr_installed(addr),
            "IP should be removed after parent drops"
        );

        let _ = std::fs::remove_file(&ready);
        let _ = std::fs::remove_file(&release);
    }

    /// Helper invoked as a child process by `cross_process_refcount`.
    /// Skipped silently when the env-var guard is absent (normal test runs).
    #[test]
    #[cfg(target_os = "illumos")]
    fn helper_cross_process_child() {
        let addr_str = match std::env::var("LOOPBACK_HELPER_ADDR") {
            Ok(s) => s,
            Err(_) => return,
        };
        let ready = std::env::var("LOOPBACK_HELPER_READY").unwrap();
        let release = std::env::var("LOOPBACK_HELPER_RELEASE").unwrap();

        let addr: IpAddr = addr_str.parse().unwrap();
        let mgr =
            Arc::new(Mutex::new(LoopbackIpManager::new("lo0", nop_logger())));
        let _alloc =
            LoopbackIpManager::allocate(Arc::clone(&mgr), &[addr]).unwrap();

        // Signal parent that the IP is installed and we're holding the flock.
        File::create(&ready).unwrap();

        // Wait for the parent's release signal.
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if std::path::Path::new(&release).exists() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "helper timed out waiting for release signal"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        // _alloc dropped here → IP uninstalled (we're the only holder).
    }
}
