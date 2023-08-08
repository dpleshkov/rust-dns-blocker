use std::cmp::{min};
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::sync::{Arc, Mutex};
use std::collections::{HashMap};
use std::io;
use rand;
use tokio::sync::{oneshot, broadcast};

async fn process_socket(socket: TcpStream, mut resolved_rx: broadcast::Receiver<Vec<u8>>, unresolved_tx: broadcast::Sender<Vec<u8>>) -> io::Result<()> {
    let (mut socket_receive, mut socket_send) = socket.into_split();
    let read_size = 512;
    let mut msg_length_buf = vec![0u8; 2];
    let mut reader_buf = vec![0u8; read_size];
    let id_map: Arc<Mutex<HashMap<u16, u16>>> = Arc::new(Mutex::new(HashMap::new()));

    let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<u8>();

    let id_map_ = Arc::clone(&id_map);
    tokio::spawn(async move {
        loop {
            tokio::select! {
                Ok(mut buf) = resolved_rx.recv() => {
                    let id = ((buf[2] as u16) << 8) + buf[3] as u16;
                    // Wacky control flow as we cannot hold locks across awaits
                    // and compiler is finicky about detecting drop()
                    let mut send = false;
                    {
                        let guard = id_map_.lock().unwrap();
                        if let Some(used_id) = guard.get(&id) {
                            buf[3] = (used_id & 0xff) as u8;
                            buf[2] = ((used_id & 0xff00) >> 8) as u8;
                            send = true;
                        }
                    }
                    if send {
                        socket_send.write_all(&buf).await.expect("Failure sending TCP response");
                    }
                }
                _ = &mut shutdown_rx => {
                    return;
                }
            }
        }
    });

    loop {
        let mut buf: Vec<u8> = vec![];

        // Read the 2-byte length header
        let l = socket_receive.read(&mut msg_length_buf).await.expect("Failed reading TCP");
        if l < 2 {
            // connection is probably closed, no need to do anything anymore
            shutdown_tx.send(0).expect("Failure sending shutdown request to secondary task");
            return Ok(());
        }
        let msg_len = (((msg_length_buf[0] as u16) << 8) + msg_length_buf[1] as u16) as usize;
        if msg_len < 2 {
            shutdown_tx.send(1).expect("Failure sending shutdown request to secondary task");
            return Err(io::Error::new(io::ErrorKind::InvalidData, "!"));
        }
        buf.extend_from_slice(&msg_length_buf);

        // Read the rest of the message
        let mut bytes_read = 0;
        while bytes_read < msg_len {
            let read_amt = min(read_size, msg_len - bytes_read);
            let l = socket_receive.read_exact(&mut reader_buf[..read_amt]).await.expect("Failed reading TCP");
            if l == 0 {
                shutdown_tx.send(1).expect("Failure sending shutdown request to secondary task");
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "End of TCP stream when decoding DNS message"));
            }
            bytes_read += l;
            buf.extend_from_slice(&reader_buf[..read_amt]);
        }

        // Generate a new random ID
        let used_id = ((buf[2] as u16) << 8) + buf[3] as u16;
        let mut rand_id = rand::random::<u16>();
        {
            let mut guard = id_map.lock().unwrap();
            while guard.contains_key(&rand_id) {
                rand_id = rand::random::<u16>();
            }
            guard.insert(rand_id, used_id);
        }
        buf[3] = (rand_id & 0xff) as u8;
        buf[2] = ((rand_id & 0xff00) >> 8) as u8;
        unresolved_tx.send(buf).expect("failed to broadcast user query");
    }
}

async fn manage_nameserver_connection(unresolved_tx: broadcast::Sender<Vec<u8>>, resolved_tx: broadcast::Sender<Vec<u8>>) {
    let mut unresolved_rx = unresolved_tx.subscribe();

    let (socket_tx, _) = broadcast::channel::<Vec<u8>>(32);
    let (mut ready_tx, mut ready_rx) = oneshot::channel::<u8>();
    let mut socket_manager = tokio::spawn(process_resolved(socket_tx.subscribe(), resolved_tx.clone(), ready_tx));

    ready_rx.await.expect("failure awaiting readiness");
    while let Ok(buf) = unresolved_rx.recv().await {
        if socket_manager.is_finished() {
            (ready_tx, ready_rx) = oneshot::channel();
            socket_manager = tokio::spawn(process_resolved(socket_tx.subscribe(), resolved_tx.clone(), ready_tx));
            (&mut ready_rx).await.expect("failure awaiting readiness");
            socket_tx.send(buf).expect("failure communicating to socket manager");
        } else {
            //(&mut ready_rx).await.expect("failure awaiting readiness");
            socket_tx.send(buf).expect("failure communicating to socket manager");
        }
    }
}

async fn process_resolved(mut unresolved_rx: broadcast::Receiver<Vec<u8>>, resolved_tx: broadcast::Sender<Vec<u8>>, ready_tx: oneshot::Sender<u8>) -> io::Result<()> {

    /*let mut nameserver_rx: Option<OwnedReadHalf> = None;
    let mut nameserver_tx: Option<OwnedWriteHalf> = None;
    while let Ok(buf) = unresolved_rx.recv().await {
        match &mut nameserver_tx {
            None => {
                let (rx, tx) = TcpStream::connect("1.1.1.1:53").await?.into_split();
                nameserver_rx = Some(rx);
                nameserver_tx = Some(tx);

                nameserver_tx.unwrap().write_all(&buf).await.expect("error writing tcp message to nameserver");
            }
            Some(tx) => {
                tx.write_all(&buf).await.expect("error writing tcp message");
            }
        }
    }*/

    let socket = TcpStream::connect("1.1.1.1:53").await?;
    let (mut socket_receive, mut socket_send) = socket.into_split();
    let read_size = 512;
    let mut msg_length_buf = vec![0u8; 2];
    let mut reader_buf = vec![0u8; read_size];

    ready_tx.send(0).expect("failure sending ready message");

    let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<u8>();

    tokio::spawn(async move {
        loop {
            tokio::select! {
            Ok(buf) = unresolved_rx.recv() => {
                socket_send.write_all(&buf).await.expect("Failure sending query");
            }
            _ = &mut shutdown_rx => {
                return;
            }
        }
        }
    });

    loop {
        let mut buf: Vec<u8> = vec![];

        // Read the 2-byte length header
        let l = socket_receive.read(&mut msg_length_buf).await.expect("Failed reading TCP");
        if l < 2 {
            // connection is closed. this is bad
            shutdown_tx.send(0).expect("failure shutting down secondary thread");
            return Err(io::Error::new(io::ErrorKind::ConnectionAborted, "disconnected from resolver"));
        }
        let msg_len = (((msg_length_buf[0] as u16) << 8) + msg_length_buf[1] as u16) as usize;
        if msg_len < 2 {
            shutdown_tx.send(0).expect("failure shutting down secondary thread");
            return Err(io::Error::new(io::ErrorKind::InvalidData, "!"));
        }
        buf.extend_from_slice(&msg_length_buf);

        // Read the rest of the message
        let mut bytes_read = 0;
        while bytes_read < msg_len {
            let read_amt = min(read_size, msg_len - bytes_read);
            let l = socket_receive.read_exact(&mut reader_buf[..read_amt]).await.expect("Failed reading TCP");
            if l == 0 {
                shutdown_tx.send(0).expect("failure shutting down secondary thread");
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "End of TCP stream when decoding DNS message"));
            }
            bytes_read += l;
            buf.extend_from_slice(&reader_buf[..read_amt]);
        }

        resolved_tx.send(buf).expect("Error broadcasting resolved message");
    }
}

pub async fn listen() -> io::Result<()> {
    let listener = TcpListener::bind("0.0.0.0:53").await?;
    let (unresolved_tx, _) = broadcast::channel(32);
    let (resolved_tx, _) = broadcast::channel(32);

    tokio::spawn(manage_nameserver_connection(unresolved_tx.clone(), resolved_tx.clone()));

    loop {
        let (socket, _) = listener.accept().await?;
        let resolved_rx_ = resolved_tx.subscribe();
        let unresolved_tx_ = unresolved_tx.clone();
        tokio::spawn(async move {
            process_socket(socket, resolved_rx_, unresolved_tx_).await.expect("Error processing TCP connection");
        });
    }
}