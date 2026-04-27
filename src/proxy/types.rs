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
    },
    Redirect {
        status: u16,
        url: String,
    },
}
