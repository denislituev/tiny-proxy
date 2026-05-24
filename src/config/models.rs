use std::collections::HashMap;

#[cfg(feature = "api")]
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
#[cfg_attr(feature = "api", derive(Serialize, Deserialize))]
pub struct Config {
    pub sites: HashMap<String, SiteConfig>,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "api", derive(Serialize, Deserialize))]
pub struct SiteConfig {
    pub address: String,
    pub directives: Vec<Directive>,
    /// TLS configuration for this site. When present, the site listens as HTTPS.
    #[cfg_attr(feature = "api", serde(skip_serializing_if = "Option::is_none"))]
    pub tls: Option<TlsConfig>,
}

/// TLS configuration for a site — paths to certificate chain and private key.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "api", derive(Serialize, Deserialize))]
pub struct TlsConfig {
    pub cert_path: String,
    pub key_path: String,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "api", derive(Serialize, Deserialize))]
pub enum Directive {
    ReverseProxy {
        to: String,
        connect_timeout: Option<u64>,
        read_timeout: Option<u64>,
    },
    HandlePath {
        pattern: String,
        directives: Vec<Directive>,
    },
    UriReplace {
        find: String,
        replace: String,
    },
    Header {
        name: String,
        value: Option<String>,
    },
    Method {
        methods: Vec<String>,
        directives: Vec<Directive>,
    },
    StripPrefix {
        prefix: String,
    },
    Redirect {
        status: u16,
        url: String,
    },
    Respond {
        status: u16,
        body: String,
    },
}
