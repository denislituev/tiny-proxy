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
    },
    Redirect {
        status: u16,
        url: String,
    },
}
