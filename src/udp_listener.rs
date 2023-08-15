use std::collections::HashMap;
use std::io;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc};
use std::sync::{Arc, Mutex};
use std::net::SocketAddr;

async fn receiver(user_map: Arc<Mutex<HashMap<u16, (u16, SocketAddr)>>>, socket: Arc<UdpSocket>, mut rx: mpsc::Receiver<Vec<u8>>) -> io::Result<()> {
    loop {
        match rx.recv().await {
            Some(mut buf) => {
                let id: u16 = ((buf[0] as u16) << 8) + buf[1] as u16;
                let address: SocketAddr;
                {
                    let mut guard = user_map.lock().unwrap();
                    if !guard.contains_key(&id) {
                        continue;
                    }
                    let user = guard.get(&id).unwrap();
                    buf[1] = (user.0 & 0xff) as u8;
                    buf[0] = ((user.0 & 0xff00) >> 8) as u8;
                    address = user.1;
                    // we no longer need user, unlock mutex
                    guard.remove(&id);
                }
                socket.send_to(&buf, address).await?;
            }
            None => {
                return Ok(());
            }
        }
    }
}


pub async fn listen(resolver_tx: mpsc::Sender<(Vec<u8>, mpsc::Sender<Vec<u8>>)>) -> io::Result<()> {
    let (tx, rx) = mpsc::channel::<Vec<u8>>(32);
    let socket = Arc::new(UdpSocket::bind("0.0.0.0:53").await?);
    let user_map: Arc<Mutex<HashMap<u16, (u16, SocketAddr)>>> = Arc::new(Mutex::new(HashMap::new()));

    tokio::spawn(receiver(Arc::clone(&user_map), Arc::clone(&socket), rx));

    loop {
        let mut buf = vec![0u8; 512];
        let (len, address) = socket.recv_from(&mut buf).await?;
        if len >= 2 {
            let used_id = ((buf[0] as u16) << 8) + buf[1] as u16;
            let user = (used_id, address);
            let mut rand_id = rand::random::<u16>();
            {
                let mut guard = user_map.lock().unwrap();
                while guard.contains_key(&rand_id) {
                    rand_id = rand::random::<u16>();
                }
                guard.insert(rand_id, user);
            }
            buf[1] = (rand_id & 0xff) as u8;
            buf[0] = ((rand_id & 0xff00) >> 8) as u8;
            resolver_tx.send((buf, tx.clone())).await.expect("Error sending message");
        }
    }
}