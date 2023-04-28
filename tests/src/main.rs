#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _dir = std::env::args().nth(1).expect("missing arg");

    println!("Hello, world!");

    Ok(())
}
