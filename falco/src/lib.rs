use nix::sys::time::TimeSpec;
use nix::time::{clock_settime, ClockId};
use sntpc::{NtpContext, NtpTimestampGenerator, NtpUdpSocket};
use std::net::{ToSocketAddrs, UdpSocket};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("NTP error: {0:?}")]
    Ntp(sntpc::Error),
    #[error("System clock error: {0}")]
    Clock(#[from] nix::errno::Errno),
    #[error("command failed: {0}")]
    Command(String),
}

#[derive(Copy, Clone)]
struct StdTimestampGen {
    duration: std::time::Duration,
}

impl NtpTimestampGenerator for StdTimestampGen {
    fn init(&mut self) {
        self.duration = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap();
    }

    fn timestamp_sec(&self) -> u64 {
        self.duration.as_secs()
    }

    fn timestamp_subsec_micros(&self) -> u32 {
        self.duration.subsec_micros()
    }
}

#[derive(Debug)]
struct StdUdpSocket(UdpSocket);

impl NtpUdpSocket for StdUdpSocket {
    fn send_to<T: ToSocketAddrs>(
        &self,
        buf: &[u8],
        addr: T,
    ) -> sntpc::Result<usize> {
        self.0.send_to(buf, addr).map_err(|_| sntpc::Error::Network)
    }

    fn recv_from(
        &self,
        buf: &mut [u8],
    ) -> sntpc::Result<(usize, std::net::SocketAddr)> {
        self.0.recv_from(buf).map_err(|_| sntpc::Error::Network)
    }
}

pub fn time_sync(ntp_server: &str) -> Result<(), Error> {
    tracing::info!(server = %ntp_server, "synchronizing time");

    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;

    let ntp_socket = StdUdpSocket(socket);
    let context = NtpContext::new(StdTimestampGen {
        duration: std::time::Duration::ZERO,
    });

    let result = sntpc::get_time(ntp_server, &ntp_socket, context)
        .map_err(Error::Ntp)?;

    let ntp_secs = result.sec() as i64;
    let ntp_nsecs =
        (result.sec_fraction() as i64) * 1_000_000_000 / (u32::MAX as i64 + 1);
    let ts = TimeSpec::new(ntp_secs, ntp_nsecs);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap();
    let ntp_duration =
        std::time::Duration::new(ntp_secs as u64, ntp_nsecs as u32);
    let correction = if ntp_duration > now {
        ntp_duration - now
    } else {
        now - ntp_duration
    };
    let direction = if ntp_duration > now { "+" } else { "-" };

    tracing::info!(
        correction = %format!("{}{}", direction, humantime::format_duration(correction)),
        "setting system clock"
    );
    clock_settime(ClockId::CLOCK_REALTIME, ts)?;

    Ok(())
}

const POOL_NAME: &str = "fpool";

fn pool_exists() -> Result<bool, Error> {
    use std::process::Command;

    let output = Command::new("zpool")
        .args(["list", POOL_NAME])
        .output()?;

    Ok(output.status.success())
}

#[derive(Debug)]
struct DiskInfo {
    name: String,
    size: u64,
}

fn get_available_disks() -> Result<Vec<DiskInfo>, Error> {
    use std::process::Command;

    // Get all disks from diskinfo (-p for parseable/plain numbers)
    let output = Command::new("diskinfo").arg("-p").output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Command(format!("diskinfo: {}", stderr.trim())));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut all_disks: Vec<DiskInfo> = Vec::new();

    for line in stdout.lines().skip(1) {
        // diskinfo output: TYPE DISK VID PID SIZE REMOVABLE SOLID_STATE
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 5 && parts[0] == "NVME" {
            let name = parts[1].to_string();
            if let Ok(size) = parts[4].parse::<u64>() {
                all_disks.push(DiskInfo { name, size });
            }
        }
    }

    // Get disks currently in use by zpools
    let output = Command::new("zpool")
        .args(["status", "-P"])
        .output()?;
    let zpool_output = String::from_utf8_lossy(&output.stdout);

    // Filter out disks that are in use
    let available: Vec<DiskInfo> = all_disks
        .into_iter()
        .filter(|disk| !zpool_output.contains(&disk.name))
        .collect();

    Ok(available)
}

fn select_disks(available: Vec<DiskInfo>, min_size: u64) -> Result<Vec<String>, Error> {
    let mut disks = available;
    // Sort by size descending to use fewer, larger disks
    disks.sort_by(|a, b| b.size.cmp(&a.size));

    let mut selected = Vec::new();
    let mut total_size: u64 = 0;

    for disk in disks {
        selected.push(disk.name);
        total_size += disk.size;
        if total_size >= min_size {
            break;
        }
    }

    if total_size < min_size {
        return Err(Error::Command(format!(
            "not enough disk space: need {}, available {}",
            bytesize::ByteSize(min_size),
            bytesize::ByteSize(total_size)
        )));
    }

    Ok(selected)
}

pub fn init_pool(min_size: bytesize::ByteSize) -> Result<(), Error> {
    use std::process::Command;

    if pool_exists()? {
        tracing::info!("pool '{}' already exists, skipping creation", POOL_NAME);
        return Ok(());
    }

    tracing::info!(%min_size, "finding available disks");

    let available = get_available_disks()?;
    tracing::debug!(?available, "found available disks");

    let disks = select_disks(available, min_size.0)?;
    tracing::info!(?disks, "creating zpool");

    let output = Command::new("zpool")
        .args(["create", "-o", "ashift=12", "-f", POOL_NAME])
        .args(&disks)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Command(format!("zpool create: {}", stderr.trim())));
    }

    let dataset = format!("{}/falcon", POOL_NAME);
    tracing::info!(dataset, "creating zfs filesystem");

    let output = Command::new("zfs")
        .args(["create", &dataset])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Command(format!("zfs create: {}", stderr.trim())));
    }

    Ok(())
}
