use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper_tls::HttpsConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tracing::{error, info};

use crate::config::Config;
use crate::proxy::handler::proxy;

/// Start the proxy server on the specified address
pub async fn start_proxy(
    addr: SocketAddr,
    config: Config,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind(&addr).await?;

    let https = HttpsConnector::new();
    let client = Client::builder(hyper_util::rt::TokioExecutor::new()).build::<_, Incoming>(https);

    info!("Tiny Proxy listening on http://{}", addr);

    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let client = client.clone();
        let config = config.clone();

        tokio::task::spawn(async move {
            let service = service_fn(move |req| {
                let client = client.clone();
                proxy(req, client, config.clone())
            });

            if let Err(err) = hyper::server::conn::http1::Builder::new()
                .serve_connection(io, service)
                .await
            {
                error!("Error serving connection: {:?}", err);
            }
        });
    }
}
