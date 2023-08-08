use tokio::net::{UdpSocket};
//use tokio::sync::{Mutex};
use std::io;
use std::net::{Ipv4Addr};
use std::collections::HashMap;
use std::sync::{Arc, Mutex} ;

mod udp_resolver;
mod tcp_resolver;

#[tokio::main]
async fn main() -> io::Result<()> {
    let user_map: udp_resolver::SharedUserMap = Arc::new(Mutex::new(HashMap::new()));

    let to_users: udp_resolver::SharedUdpSocket = Arc::new(UdpSocket::bind("0.0.0.0:53").await?);
    let to_resolver: udp_resolver::SharedUdpSocket = Arc::new(UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?);
    to_resolver.connect("1.1.1.1:53").await?;

    // Make copies of our Arcs to give to UDP polling task
    let to_users_ = Arc::clone(&to_users);
    let to_resolver_ = Arc::clone(&to_resolver);
    let user_map_ = Arc::clone(&user_map);

    tokio::spawn(async move {
        udp_resolver::listen_for_resolved_queries(to_users_, to_resolver_, user_map_).await.expect("Error");
    });

    tokio::spawn(async move {
        tcp_resolver::listen().await.expect("Error in TCP task");
    });

    udp_resolver::listen_for_new_queries(to_users, to_resolver, user_map).await.expect("Error");

    return Ok(());
}
