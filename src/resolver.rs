use std::io;
use std::sync::Arc;
use tokio::sync::{mpsc};
use hyper::{Body, Client, Request, Method};
use hyper::client::HttpConnector;
use hyper_rustls;
use hyper_rustls::HttpsConnector;
use simple_dns;
use simple_dns::RCODE::NameError;


async fn resolve(client: Client<HttpsConnector<HttpConnector>, Body>, message: Vec<u8>, tx: mpsc::Sender<Vec<u8>>, filter: Arc<crate::filter::Filter>) -> io::Result<()> {
    let res = simple_dns::Packet::parse(&message);
    if !res.is_ok() {
        // echo the bad packet back
        tx.send(message).await.expect("Failure returning response");
        return Ok(())
    }
    let packet = res.unwrap();
    for question in packet.questions.iter() {
        if filter.contains(&question.qname.to_string()) {
            let mut reply = packet.clone().into_reply();
            let rcode = reply.rcode_mut();
            *rcode = NameError;
            tx.send(reply.build_bytes_vec_compressed().expect("Failure building response")).await.expect("Failure returning response");
            return Ok(());
        }
    }
    let req = Request::builder()
        .method(Method::POST)
        .uri("https://1.1.1.1/dns-query")
        .header("content-type", "application/dns-message")
        .body(Body::from(message)).expect("Error constructing request");

    let resp = client.request(req).await.expect("Error sending request");
    let data = hyper::body::to_bytes(resp.into_body()).await.expect("Failure parsing response");
    tx.send(data.to_vec()).await.expect("Failure returning response");
    Ok(())
}

pub async fn resolver(mut rx: mpsc::Receiver<(Vec<u8>, mpsc::Sender<Vec<u8>>)>, filter: Arc<crate::filter::Filter>) -> io::Result<()> {
    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_native_roots()
        .https_only()
        .enable_http2()
        .build();

    let client: Client<_, Body> = Client::builder().build(https);

    loop {
        if let Some(message) = rx.recv().await {
            tokio::spawn(resolve(client.clone(), message.0, message.1, Arc::clone(&filter)));
        }
    }
}