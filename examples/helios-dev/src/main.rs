use libfalcon::{cli::run, error::Error, unit::gb, Runner};

#[tokio::main]
async fn main() -> Result<(), Error> {
    let mut d = Runner::new("hdev");

    // Modify cores or memory if you'd like
    let masaka = d.node("masaka", "helios-1.1", 4, gb(4));
    d.mount("./cargo-bay", "/opt/cargo-bay", masaka)?;

    // XXX Change this to point at your host machine's internet-facing network
    // interface.
    d.ext_link("igb0", masaka);

    run(&mut d).await?;

    Ok(())
}
