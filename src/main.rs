// Uncomment this block to pass the first stage
use std::io::Result;

mod listener;
mod options;

#[tokio::main]
async fn main() -> Result<()> {
    let options = options::ServerOptions::new();
    let handle = listener::start("127.0.0.1:4221", options);
    handle.await.unwrap()
}
