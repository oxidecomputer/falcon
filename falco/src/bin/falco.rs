use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "falco")]
#[command(about = "Falcon utilities")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Synchronize system time from an NTP server
    TimeSync {
        /// NTP server address
        #[arg(short, long, default_value = "pool.ntp.org:123")]
        server: String,
    },
    /// Initialize a zpool with a falcon filesystem
    InitPool {
        /// Minimum pool size (e.g., "100G", "1T", "500GiB")
        size: bytesize::ByteSize,
    },
}

fn main() {
    tracing_subscriber::fmt().without_time().with_target(false).init();
    let cli = Cli::parse();

    match cli.command {
        Commands::TimeSync { server } => {
            if let Err(e) = falco::time_sync(&server) {
                tracing::error!(%e, "time sync failed");
                std::process::exit(1);
            }
            tracing::info!("time synchronized successfully");
        }
        Commands::InitPool { size } => {
            if let Err(e) = falco::init_pool(size) {
                tracing::error!(%e, "init pool failed");
                std::process::exit(1);
            }
            tracing::info!("pool initialized successfully");
        }
    }
}
