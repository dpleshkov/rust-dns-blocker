use tokio::sync::{mpsc};
use std::io;

mod dns_over_https;
mod udp_listener;
mod tcp_listener;
// mod dot_listener;
mod doh_listener;

#[tokio::main]
async fn main() -> io::Result<()> {
    let (tx, rx) = mpsc::channel::<(Vec<u8>, mpsc::Sender<Vec<u8>>)>(32);

    tokio::spawn(udp_listener::listen(tx.clone()));
    tokio::spawn(tcp_listener::listen(tx.clone()));
    tokio::spawn(doh_listener::listen(tx.clone()));

    dns_over_https::resolver(rx).await?;

    return Ok(());
}
