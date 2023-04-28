#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let dir = env::args().nth(1).expect("missing arg");

    println!("Hello, world!");

    Ok(())
}
