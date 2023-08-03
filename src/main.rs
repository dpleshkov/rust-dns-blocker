use tokio::net::{UdpSocket, TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use dashmap::DashMap;
use std::sync::{Arc};
//use simple_dns::*;
//use simple_dns::rdata::*;
use parking_lot::Mutex;


struct User {
    address: SocketAddr,
    used_id: u16
}

struct UserTcp {
    socket: SharedTcpStream,
    used_id: u16
}

type Users = Arc<DashMap<u16, User>>;
type UsersTcp = Arc<DashMap<u16, UserTcp>>;
type SharedUdpSocket = Arc<UdpSocket>;
type SharedTcpStream = Arc<Mutex<TcpStream>>;
type SharedTcpListener = Arc<TcpListener>;



async fn poll_resolved_packets(inbound: SharedUdpSocket, outbound: SharedUdpSocket, users: Users) -> io::Result<()> {
    loop {
        // Receive packet from actual DNS service
        let mut buf = vec![0u8; 512];
        let len = outbound.recv(&mut buf).await?;
        if len >= 2 {
            let id: u16 = ((buf[0] as u16) << 8) + buf[1] as u16;
            println!("received outbound for id {id}");
            if let Some(user) = users.get(&id) {
                buf[1] = (user.used_id & 0xff) as u8;
                buf[0] = ((user.used_id & 0xff00) >> 8) as u8;
                let uid = user.used_id;
                println!("{:?}", &buf[..len]);
                println!("Received {len} bytes response for id {id}. Giving it to ID {uid}.");
                inbound.send_to(&buf[..len], user.address).await?;
                users.remove(&id);
            }
        } 
    }
}

async fn tcp_process_socket(socket: SharedTcpStream, outbound_tcp: SharedTcpStream, users_tcp: UsersTcp) -> io::Result<()> {
    loop {
        let mut buf = vec![0u8; 1024];
        let n = socket.lock().read(&mut buf).await.expect("Failed reading data from TCPStream");
        if n == 0 {
            ();
        }
        let used_id = ((buf[0] as u16) << 8) + buf[1] as u16;
        let mut rand_id = rand::random::<u16>();
        while users_tcp.contains_key(&rand_id) {
            rand_id = rand::random::<u16>();
        }
        let address = socket.lock().peer_addr()?;
        let socket_: SharedTcpStream = Arc::clone(&socket);
        let user = UserTcp {socket: socket_, used_id};
        println!("Received req from TCP {address} with provided id {used_id}. Giving it ID {rand_id}.");
        users_tcp.insert(rand_id, user);
        buf[1] = (rand_id & 0xff) as u8;
        buf[0] = ((rand_id & 0xff00) >> 8) as u8;
        outbound_tcp.lock().write_all(&buf[..n]).await.expect("Failed to send data to outbound");
    }
}

async fn tcp_listen_resolved(socket: SharedTcpStream, users_tcp: UsersTcp) -> io::Result<()> {
    loop {
        let mut buf = vec![0u8; 1024];
        let n = socket.lock().read(&mut buf).await.expect("Failed to read from resolving stream");
        if n == 0 {
            ();
        }
        let id = ((buf[0] as u16) << 8) + buf[1] as u16;
        println!("received tcp outbound for id {id}");
        if let Some(user) = users_tcp.get(&id) {
            buf[1] = (user.used_id & 0xff) as u8;
            buf[0] = ((user.used_id & 0xff00) >> 8) as u8;
            let uid = user.used_id;
            println!("Received response for id {id}. Giving it to ID {uid}.");
            user.socket.lock().write_all(&buf[..n]).await?;
            users_tcp.remove(&id);
        }
    }
}

async fn tcp_listen_for_incoming(listener: SharedTcpListener, outbound_tcp: SharedTcpStream) -> io::Result<()> {
    let users_tcp: UsersTcp = Arc::new(DashMap::new());

    let users_tcp_ = Arc::clone(&users_tcp);
    let outbound_tcp_ = Arc::clone(&outbound_tcp);
    tokio::spawn(async move {
        tcp_listen_resolved(outbound_tcp_, users_tcp_).await.expect("Failure resolving tcp queries");
    });

    loop {
        let (socket, _) = listener.accept().await?;
        let socket_ = Arc::new(Mutex::new(socket));

        let users_tcp_ = Arc::clone(&users_tcp);
        let outbound_tcp_ = Arc::clone(&outbound_tcp);
        tokio::spawn(async move {
            tcp_process_socket(socket_, outbound_tcp_, users_tcp_).await.expect("Failure with TCP socket");
        });
    }
}


#[tokio::main]
async fn main() -> io::Result<()> {
    let users: Users = Arc::new(DashMap::new());

    let inbound_udp: SharedUdpSocket = Arc::new(UdpSocket::bind("0.0.0.0:53").await?);
    let outbound_udp: SharedUdpSocket = Arc::new(UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?);
    outbound_udp.connect("1.1.1.1:53").await?;

    let inbound_tcp: SharedTcpListener = Arc::new(TcpListener::bind("0.0.0.0:53").await?);
    let outbound_tcp: SharedTcpStream = Arc::new(Mutex::new(TcpStream::connect("1.1.1.1:53").await?));

    // Make copies of our Arcs to give to UDP polling task
    let inbound_ = Arc::clone(&inbound_udp);
    let outbound_ = Arc::clone(&outbound_udp);
    let users_ = Arc::clone(&users);

    tokio::spawn(async move {
        poll_resolved_packets(inbound_, outbound_, users_).await.expect("Could not spawn resolving task!");
    });

    // Do the same for TCP polling task
    /*let inbound_tcp_ = Arc::clone(&inbound_tcp);
    let outbound_tcp_ = Arc::clone(&outbound_tcp);

    tokio::spawn(async move {
        tcp_listen_for_incoming(inbound_tcp_, outbound_tcp_).await.expect("Error in TCP listener loop");
    });*/


    loop {
        println!("Hello");
        let mut buf = vec![0u8; 512];
        let (len, address) = inbound_udp.recv_from(&mut buf).await?;
        println!("Received {len} bytes");
        if len >= 2 {
            let used_id = ((buf[0] as u16) << 8) + buf[1] as u16;
            let user = User {
                address,
                used_id
            };
            let mut rand_id = rand::random::<u16>();
            while users.contains_key(&rand_id) {
                rand_id = rand::random::<u16>();
            }
            println!("{:?}", &buf[..len]);
            println!("Received {len} bytes req from {address} with provided id {used_id}. Giving it ID {rand_id}.");
            users.insert(rand_id, user);
            buf[1] = (rand_id & 0xff) as u8;
            buf[0] = ((rand_id & 0xff00) >> 8) as u8;
            outbound_udp.send(&buf[..len]).await?;
            println!("End loop");
        }
    }
}
