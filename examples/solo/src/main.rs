use libfalcon::{cli::run, error::Error, unit::gb, Runner};

#[tokio::main]
async fn main() -> Result<(), Error> {
    let mut d = Runner::new("solo");

    d.node("violin", "helios-1.1", 2, gb(2));
    run(&mut d).await?;
    Ok(())
}
