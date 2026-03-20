use hyper::body::Incoming;
use hyper::Request;
use tracing::info;

use crate::proxy::ActionResult;

/// Handle reverse_proxy directive
pub fn handle_reverse_proxy(to: &str, path: &str) -> ActionResult {
    info!("   Proxying to: {}", to);
    ActionResult::ReverseProxy {
        backend_url: to.to_string(),
        path_to_send: path.to_string(),
    }
}

/// Handle respond directive
pub fn handle_respond(status: &u16, body: &str) -> ActionResult {
    info!("   Returning direct response: {}", status);
    ActionResult::Respond {
        status: *status,
        body: body.to_string(),
    }
}

/// Handle header directive - add or replace header in request
pub fn handle_header(name: &str, value: &str, req: &mut Request<Incoming>) -> anyhow::Result<()> {
    use hyper::header::{HeaderName, HeaderValue};

    let header_name = HeaderName::from_bytes(name.as_bytes())?;
    let header_value = HeaderValue::from_str(value)?;

    req.headers_mut().insert(header_name, header_value);
    info!("   Applied header: {}", name);

    Ok(())
}

/// Handle uri_replace directive - replace substring in path
pub fn handle_uri_replace(find: &str, replace: &str, path: &mut String) {
    *path = path.replace(find, replace);
    info!("   Applied uri_replace: {} → {}", find, replace);
}

/// Handle method directive - check if request method matches allowed methods
pub fn handle_method(methods: &[String], req: &Request<Incoming>) -> bool {
    methods
        .iter()
        .any(|m| m.eq_ignore_ascii_case(req.method().as_str()))
}
