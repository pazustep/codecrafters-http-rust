// Uncomment this block to pass the first stage
use std::io::Result;

mod listener;

#[tokio::main]
async fn main() -> Result<()> {
    let handle = listener::start("127.0.0.1:4221");
    handle.await.unwrap()
}
