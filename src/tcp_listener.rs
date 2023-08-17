use std::cmp::min;
use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::sync::{mpsc, oneshot};

async fn callback_loop(mut socket: OwnedWriteHalf, mut callback_rx: mpsc::Receiver<Vec<u8>>, mut shutdown_rx: oneshot::Receiver<()>, id_map: Arc<Mutex<HashMap<u16, u16>>>) -> io::Result<()> {
    let mut len_buf = vec![0u8; 2];
    loop {
        tokio::select! {
            _ = &mut shutdown_rx => {
                return Ok(());
            }
            res = callback_rx.recv() => {
                if let Some(mut buf) = res {
                    let id = ((buf[0] as u16) << 8) + buf[1] as u16;
                    {
                        let guard = id_map.lock().unwrap();
                        if let Some(used_id) = guard.get(&id) {
                            buf[1] = (used_id & 0xff) as u8;
                            buf[0] = ((used_id & 0xff00) >> 8) as u8;
                        }
                    }
                    let length = buf.len();
                    len_buf[0] = ((length & 0xff00) >> 8) as u8;
                    len_buf[1] = (length & 0xff) as u8;
                    socket.write_all(&len_buf).await.expect("Failure writing length 2-octet to socket");
                    socket.write_all(&buf).await.expect("Failure writing data to socket");
                } else {
                    return Ok(());
                }
            }
        }
    }
}

async fn process_socket(socket: TcpStream, resolver_tx: mpsc::Sender<(Vec<u8>, mpsc::Sender<Vec<u8>>)>) -> io::Result<()> {
    let (callback_tx, callback_rx) = mpsc::channel::<Vec<u8>>(32);
    let (mut socket_rx, socket_tx) = socket.into_split();
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let id_map: Arc<Mutex<HashMap<u16, u16>>> = Arc::new(Mutex::new(HashMap::new()));

    tokio::spawn(callback_loop(socket_tx, callback_rx, shutdown_rx, Arc::clone(&id_map)));

    let read_size = 512;
    let mut msg_length_buf = vec![0u8; 2];
    let mut reader_buf = vec![0u8; read_size];

    loop {
        let mut buf: Vec<u8> = vec![];

        // Read the 2-byte length header
        let l = socket_rx.read(&mut msg_length_buf).await.expect("Failed reading TCP");
        if l < 2 {
            // connection is probably closed, no need to do anything anymore
            shutdown_tx.send(()).expect("Failure sending shutdown request to secondary task");
            return Ok(());
        }
        let msg_len = (((msg_length_buf[0] as u16) << 8) + msg_length_buf[1] as u16) as usize;
        if msg_len < 2 {
            shutdown_tx.send(()).expect("Failure sending shutdown request to secondary task");
            return Err(io::Error::new(io::ErrorKind::InvalidData, "!"));
        }
        // buf.extend_from_slice(&msg_length_buf);

        // Read the rest of the message
        let mut bytes_read = 0;
        while bytes_read < msg_len {
            let read_amt = min(read_size, msg_len - bytes_read);
            let l = socket_rx.read_exact(&mut reader_buf[..read_amt]).await.expect("Failed reading TCP");
            if l == 0 {
                shutdown_tx.send(()).expect("Failure sending shutdown request to secondary task");
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "End of TCP stream when decoding DNS message"));
            }
            bytes_read += l;
            buf.extend_from_slice(&reader_buf[..read_amt]);
        }

        // Generate a new random ID
        let used_id = ((buf[0] as u16) << 8) + buf[1] as u16;
        let mut rand_id = rand::random::<u16>();
        {
            let mut guard = id_map.lock().unwrap();
            while guard.contains_key(&rand_id) {
                rand_id = rand::random::<u16>();
            }
            guard.insert(rand_id, used_id);
        }
        buf[1] = (rand_id & 0xff) as u8;
        buf[0] = ((rand_id & 0xff00) >> 8) as u8;
        resolver_tx.send((buf, callback_tx.clone())).await.expect("failed to broadcast user query");
    }
}

pub async fn listen(nameserver_tx: mpsc::Sender<(Vec<u8>, mpsc::Sender<Vec<u8>>)>) -> io::Result<()> {
    let listener = TcpListener::bind("0.0.0.0:53").await?;
    loop {
        let (socket, _) = listener.accept().await?;
        tokio::spawn(process_socket(socket, nameserver_tx.clone()));
    }
}