use tokio::net::UdpSocket;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::net::{SocketAddr};
use std::io;
use rand;

/// Associates a DNS query ID with the IP address of the user who (allegedly) sent it
pub struct UserMapping {
    id: u16,
    address: SocketAddr
}
/// Shared HashMap mapping a randomly generated query ID with the original query ID and address of
/// the user sending it
pub type SharedUserMap = Arc<Mutex<HashMap<u16, UserMapping>>>;
/// Shared `tokio::net::UdpSocket`
pub type SharedUdpSocket = Arc<UdpSocket>;

/// Polls `from_resolver` for incoming DNS query responses, then proxies them back to the user
/// addresses found in `user_map` using the socket `to_users`
pub async fn listen_for_resolved_queries(to_users: SharedUdpSocket, from_resolver: SharedUdpSocket, user_map: SharedUserMap) -> io::Result<()> {
    let mut buf = vec![0u8; 512];
    loop {
        // Receive packet from actual DNS service
        let len = from_resolver.recv(&mut buf).await?;
        if len >= 2 {
            let id: u16 = ((buf[0] as u16) << 8) + buf[1] as u16;
            // println!("received outbound for id {id}");
            let address: SocketAddr;
            {
                let mut guard = user_map.lock().unwrap();
                if !guard.contains_key(&id) {
                    continue;
                }
                let user = guard.get(&id).unwrap();
                buf[1] = (user.id & 0xff) as u8;
                buf[0] = ((user.id & 0xff00) >> 8) as u8;
                address = user.address;
                // we no longer need user, unlock mutex
                guard.remove(&id);
            }

            to_users.send_to(&buf[..len], address).await?;
            // println!("{:?}", &buf[..len]);
            // println!("Received {len} bytes response for id {id}. Giving it to ID {uid}.");
        }
    }
}

/// Polls `from_users` for new queries, maps the requester's address and query id with another
/// random ID in `user_map` and proxies the query along to the DNS resolver with socket
/// `to_resolver`
pub async fn listen_for_new_queries(from_users: SharedUdpSocket, to_resolver: SharedUdpSocket, user_map: SharedUserMap) -> io::Result<()> {
    let mut buf = vec![0u8; 512];
    loop {
        let (len, address) = from_users.recv_from(&mut buf).await?;
        // println!("Received {len} bytes");
        if len >= 2 {
            let used_id = ((buf[0] as u16) << 8) + buf[1] as u16;
            let user = UserMapping {
                address,
                id: used_id
            };
            let mut rand_id = rand::random::<u16>();
            let mut guard = user_map.lock().unwrap();
            while guard.contains_key(&rand_id) {
                rand_id = rand::random::<u16>();
            }
            // println!("{:?}", &buf[..len]);
            // println!("Received {len} bytes req from {address} with provided id {used_id}. Giving it ID {rand_id}.");
            guard.insert(rand_id, user);
            drop(guard);
            buf[1] = (rand_id & 0xff) as u8;
            buf[0] = ((rand_id & 0xff00) >> 8) as u8;
            to_resolver.send(&buf[..len]).await?;
        }
    }
}