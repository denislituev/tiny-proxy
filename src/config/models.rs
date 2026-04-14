use std::collections::HashMap;

#[cfg(feature = "api")]
use serde::{Deserialize, Serialize};

// Models remain as same as we designed
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
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "api", derive(Serialize, Deserialize))]
pub enum Directive {
    ReverseProxy {
        to: String,
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
        value: String,
    },
    Method {
        methods: Vec<String>,
        directives: Vec<Directive>,
    },
    Respond {
        status: u16,
        body: String,
    },
}
