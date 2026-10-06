use bytes::Bytes;
use hyper::header::HeaderValue;
use hyper_rustls::HttpsConnector;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;

use crate::config::HeaderDirective;

/// Unified request body type for outbound requests sent through the shared
/// pooled client. Backend proxying streams the client's [`hyper::body::Incoming`]
/// body boxed into this type; auth subrequests use an empty body.
///
/// A single body type is required because `Client<C, B>` fixes `B` per instance —
/// unifying here lets backend requests and forward-auth subrequests share one
/// connection pool.
pub type ProxyRequestBody =
    http_body_util::combinators::BoxBody<Bytes, Box<dyn std::error::Error + Send + Sync>>;

/// The shared pooled HTTP client used for backend requests and forward-auth
/// subrequests alike.
pub type ProxyClient = Client<HttpsConnector<HttpConnector>, ProxyRequestBody>;

/// Result of directive processing
#[derive(Debug, Clone)]
pub enum ActionResult {
    Respond {
        status: u16,
        body: String,
    },
    ReverseProxy {
        backend_url: String,
        path_to_send: String,
        connect_timeout: Option<u64>,
        read_timeout: Option<u64>,
        header_up: Vec<HeaderDirective>,
    },
    Redirect {
        status: u16,
        url: String,
    },
    /// A `forward_auth` middleware rejected the request (401/403/other 4xx
    /// denial) or the auth service failed while `failure_mode closed` was in
    /// effect (503).
    AuthDenied {
        status: u16,
        /// Authentication challenge preserved from the auth service response
        /// (e.g. `WWW-Authenticate` on a 401). Never populated from client input.
        www_authenticate: Option<HeaderValue>,
    },
}
