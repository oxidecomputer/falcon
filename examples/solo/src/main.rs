use libfalcon::{cli::run, error::Error, unit::gb, Runner};

#[tokio::main]
async fn main() -> Result<(), Error> {
    let mut d = Runner::new("solo");

    d.node("violin", "netstack-1.2", 2, gb(2));
    run(&mut d).await?;
    Ok(())
}
