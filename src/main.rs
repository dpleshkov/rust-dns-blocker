use tokio::net::{UdpSocket};
use tokio::sync::{Mutex};
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::collections::HashMap;
use std::sync::{Arc};

struct User {
    address: SocketAddr,
    used_id: u16
}

type Users = Arc<Mutex<HashMap<u16, User>>>;
type SharedUdpSocket = Arc<UdpSocket>;

async fn udp_listen_resolved(inbound: SharedUdpSocket, outbound: SharedUdpSocket, users: Users) -> io::Result<()> {
    let mut buf = vec![0u8; 512];
    loop {
        // Receive packet from actual DNS service
        let len = outbound.recv(&mut buf).await?;
        if len >= 2 {
            let id: u16 = ((buf[0] as u16) << 8) + buf[1] as u16;
            println!("received outbound for id {id}");

            let mut guard = users.lock().await;
            if !guard.contains_key(&id) {
                continue;
            }
            let user = guard.get(&id).unwrap();
            buf[1] = (user.used_id & 0xff) as u8;
            buf[0] = ((user.used_id & 0xff00) >> 8) as u8;
            let uid = user.used_id;
            let address = user.address;
            // we no longer need user, unlock mutex
            guard.remove(&id);
            drop(guard);
            inbound.send_to(&buf[..len], address).await?;
            println!("{:?}", &buf[..len]);
            println!("Received {len} bytes response for id {id}. Giving it to ID {uid}.");
        } 
    }
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let users: Users = Arc::new(Mutex::new(HashMap::new()));

    let inbound_udp: SharedUdpSocket = Arc::new(UdpSocket::bind("0.0.0.0:53").await?);
    let outbound_udp: SharedUdpSocket = Arc::new(UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?);
    outbound_udp.connect("1.1.1.1:53").await?;

    // Make copies of our Arcs to give to UDP polling task
    let inbound_ = Arc::clone(&inbound_udp);
    let outbound_ = Arc::clone(&outbound_udp);
    let users_ = Arc::clone(&users);

    tokio::spawn(async move {
        udp_listen_resolved(inbound_, outbound_, users_).await.expect("Could not spawn resolving task!");
    });

    let mut buf = vec![0u8; 512];
    loop {
        let (len, address) = inbound_udp.recv_from(&mut buf).await?;
        println!("Received {len} bytes");
        if len >= 2 {
            let used_id = ((buf[0] as u16) << 8) + buf[1] as u16;
            let user = User {
                address,
                used_id
            };
            let mut rand_id = rand::random::<u16>();
            let mut guard = users.lock().await;
            while guard.contains_key(&rand_id) {
                rand_id = rand::random::<u16>();
            }
            println!("{:?}", &buf[..len]);
            println!("Received {len} bytes req from {address} with provided id {used_id}. Giving it ID {rand_id}.");
            guard.insert(rand_id, user);
            drop(guard);
            buf[1] = (rand_id & 0xff) as u8;
            buf[0] = ((rand_id & 0xff00) >> 8) as u8;
            outbound_udp.send(&buf[..len]).await?;
        }
    }
}
