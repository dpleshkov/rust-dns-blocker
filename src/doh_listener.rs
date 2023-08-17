use std::vec::Vec;
use std::{fs, io};
use std::net::SocketAddr;

use hyper::server::conn::AddrIncoming;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Method, Request, Response, Server, StatusCode};
use hyper_rustls::TlsAcceptor;

use tokio::sync::mpsc;

use url::Url;

use base64::{Engine as _, alphabet, engine::{self, general_purpose}};

// type BufReceiver = mpsc::Receiver<Vec<u8>>;
type BufSender = mpsc::Sender<Vec<u8>>;
type QuerySender = mpsc::Sender<(Vec<u8>, BufSender)>;

fn load_certs(filename: &str) -> io::Result<Vec<rustls::Certificate>> {
    let cert_file = fs::File::open(filename).expect("Failed to open cert file");
    let mut reader = io::BufReader::new(cert_file);
    let certs = rustls_pemfile::certs(&mut reader).expect("Failed parsing cert");

    Ok(certs.into_iter().map(rustls::Certificate).collect())
}

fn load_private_key(filename: &str) -> io::Result<rustls::PrivateKey> {
    let keyfile = fs::File::open(filename).expect("Failed to open key file");
    let mut reader = io::BufReader::new(keyfile);
    let keys = rustls_pemfile::rsa_private_keys(&mut reader).expect("Failed parsing key");
    if keys.len() != 1 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "more than one key provided"));
    }

    Ok(rustls::PrivateKey(keys[0].clone()))
}

async fn resolve(query: Vec<u8>, nameserver_tx: QuerySender) -> io::Result<Vec<u8>> {
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(1);
    nameserver_tx.send((query, tx)).await.expect("Failure sending query");
    let response = rx.recv().await.expect("Failure in inter task communication");
    return Ok(response);
}

async fn process_request(req: Request<Body>, nameserver_tx: QuerySender) -> Result<Response<Body>, hyper::Error> {
    match (req.method(), req.uri().path()) {
        (&Method::GET, "/dns-query") => {
            let url = Url::parse(&req.uri().to_string()).expect("Failure parsing URI");
            let pairs = url.query_pairs();
            for (name, value) in pairs {
                if name == "dns" {
                    let dns_query = engine::GeneralPurpose::new(
                        &alphabet::URL_SAFE,
                        general_purpose::NO_PAD)
                        .decode(value.as_ref()).unwrap();

                    let dns_response = resolve(dns_query, nameserver_tx).await.expect("Error processing response");
                    return Ok(
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "application/dns-message")
                            .body(Body::from(dns_response)).expect("Failure building body")
                    );
                }
            }
            Ok(
                Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .body(Body::from("No DNS query message passed in URI")).expect("Failure building body")
            )
        },
        (&Method::POST, "/dns-query") => {
            let dns_query = hyper::body::to_bytes(Body::from(req.into_body())).await.expect("Failure processing body");
            let dns_response = resolve(dns_query.to_vec(), nameserver_tx).await.expect("Error processing response");
            return Ok(
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/dns-message")
                    .body(Body::from(dns_response)).expect("Failure building body")
            );
        }
        _ => Ok(Response::new(Body::from(
            "This is a DNS-over-HTTPS resolver. Try querying /dns-query as per RFC 8484"
        )))
    }
}

pub async fn listen(nameserver_tx: QuerySender) -> io::Result<()> {
    let certs = load_certs("resources/public.pem").expect("Failed loading cert");
    let key = load_private_key("resources/private.pem").expect("Failed loading key");

    let address = "0.0.0.0:443".parse::<SocketAddr>().expect("Invalid address");
    let incoming = AddrIncoming::bind(&address).expect("Failure binding to address");

    let acceptor = TlsAcceptor::builder()
        .with_single_cert(certs, key)
        .expect("Error building tls acceptor")
        .with_all_versions_alpn()
        .with_incoming(incoming);

    let make_svc = make_service_fn(move |_| { // first move it into the closure
        // closure can be called multiple times, so for each call, we must
        // clone it and move that clone into the async block
        let tx = nameserver_tx.clone();
        async move {
            // async block is only executed once, so just pass it on to the closure
            Ok::<_, hyper::Error>(service_fn(move |req| {
                // but this closure may also be called multiple times, so make
                // a clone for each call, and move the clone into the async block
                let tx = tx.clone();
                async move { process_request(req, tx).await }
            }))
        }
    });


    let server = Server::builder(acceptor)
        .serve(make_svc);

    server.await.expect("Error with server");

    Ok(())
}