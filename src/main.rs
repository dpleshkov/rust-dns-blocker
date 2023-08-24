use tokio::sync::{mpsc};
use std::io;
use std::sync::Arc;

mod resolver;
mod udp_listener;
mod tcp_listener;
// mod dot_listener;
mod doh_listener;

mod filter;

#[tokio::main]
async fn main() -> io::Result<()> {
    let (tx, rx) = mpsc::channel::<(Vec<u8>, mpsc::Sender<Vec<u8>>)>(32);

    tokio::spawn(udp_listener::listen(tx.clone()));
    tokio::spawn(tcp_listener::listen(tx.clone()));
    tokio::spawn(doh_listener::listen(tx.clone()));

    let mut filter = filter::Filter::new();
    filter.add_file("./resources/ads.txt")?;

    resolver::resolver(rx, Arc::new(filter)).await?;

    return Ok(());
}
