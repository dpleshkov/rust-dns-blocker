use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};
use tokio_rustls::rustls::{ClientConfig, OwnedTrustAnchor, RootCertStore, ServerName};
use tokio_rustls::TlsConnector;
use webpki_roots;

pub async fn listen(nameserver_tx: mpsc::Sender<(Vec<u8>, mpsc::Sender<Vec<u8>>)>) {

    // Boilerplate code
    let mut root_cert_store = RootCertStore::empty();
    root_cert_store.add_trust_anchors(webpki_roots::TLS_SERVER_ROOTS.iter().map(|ta| {
        OwnedTrustAnchor::from_subject_spki_name_constraints(
            ta.subject,
            ta.spki,
            ta.name_constraints,
        )
    }));

    let config = Arc::new(
        ClientConfig::builder()
            .with_safe_defaults()
            .with_root_certificates(root_cert_store)
            .with_no_client_auth()
    );

    

}