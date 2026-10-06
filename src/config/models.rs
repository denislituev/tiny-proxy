use std::collections::HashMap;

use hyper::header::HeaderName;
use hyper::Uri;

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
        #[cfg_attr(feature = "api", serde(default))]
        header_up: Vec<HeaderDirective>,
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
    /// Forward-auth middleware: performs an authorization subrequest before
    /// the request continues down the pipeline. Evaluated in directive order,
    /// so it can appear at site level or inside `handle_path` / `method` blocks.
    ForwardAuth(ForwardAuthConfig),
}

/// A single header operation within a `header_up` block.
/// `value = None` means the header should be removed.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "api", derive(Serialize, Deserialize))]
pub struct HeaderDirective {
    pub name: String,
    pub value: Option<String>,
}

/// Behavior of `forward_auth` when the auth service cannot produce an
/// authorization decision (connection error, timeout, 5xx, malformed response).
///
/// Explicit `401`/`403` decisions are **never** bypassed by `Open`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "api", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "api", serde(rename_all = "lowercase"))]
pub enum FailureMode {
    /// Reject the request with `503 Service Unavailable` (default, secure).
    #[default]
    Closed,
    /// Let the request continue **without** identity headers when the auth
    /// service has an infrastructure failure.
    Open,
}

/// `forward_auth` middleware configuration.
///
/// The endpoint, header names, and failure mode are validated when the
/// configuration is loaded (text parser and JSON API alike) — never deferred
/// to request time.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "api", derive(Serialize, Deserialize))]
pub struct ForwardAuthConfig {
    /// Auth service endpoint. Must be an absolute `http://`/`https://` URI.
    #[cfg_attr(feature = "api", serde(rename = "to", with = "serde_impl::uri"))]
    pub endpoint: Uri,

    /// Client headers copied into the auth subrequest (case-insensitive).
    /// Everything else is **not** forwarded.
    #[cfg_attr(
        feature = "api",
        serde(
            with = "serde_impl::header_name_list",
            default = "default_request_headers"
        )
    )]
    pub request_headers: Vec<HeaderName>,

    /// Auth response headers propagated to the backend as trusted identity
    /// headers. Client-supplied values for these headers are always removed
    /// before the auth request runs (spoofing protection).
    #[cfg_attr(feature = "api", serde(with = "serde_impl::header_name_list", default))]
    pub response_headers: Vec<HeaderName>,

    /// Infrastructure-failure behavior. Explicit 401/403 denials are not
    /// affected by this setting.
    #[cfg_attr(feature = "api", serde(default))]
    pub failure_mode: FailureMode,

    /// Auth subrequest timeout in seconds.
    #[cfg_attr(feature = "api", serde(default = "default_auth_timeout_secs"))]
    pub timeout_secs: u64,
}

/// Default client headers forwarded to the auth service: credentials only.
pub fn default_request_headers() -> Vec<HeaderName> {
    vec![
        HeaderName::from_static("authorization"),
        HeaderName::from_static("cookie"),
    ]
}

/// Default auth subrequest timeout: 5 seconds.
pub fn default_auth_timeout_secs() -> u64 {
    5
}

/// Validate and normalize a forward-auth endpoint.
///
/// Requires an absolute `http`/`https` URI with an authority; fills in an
/// empty path with `/`. Used by both the text parser and the JSON API so
/// both config formats enforce the same rules.
pub fn normalize_auth_endpoint(uri: Uri) -> Result<Uri, String> {
    let scheme = uri
        .scheme_str()
        .ok_or_else(|| "missing scheme (use http:// or https://)".to_string())?;
    if scheme != "http" && scheme != "https" {
        return Err(format!("unsupported scheme '{scheme}' (use http or https)"));
    }
    if uri.authority().is_none() {
        return Err("missing host".to_string());
    }
    if uri.path_and_query().is_none() {
        let mut parts = uri.into_parts();
        parts.path_and_query = Some(
            "/".parse()
                .map_err(|e| format!("failed to set default path: {e}"))?,
        );
        return Uri::from_parts(parts).map_err(|e| e.to_string());
    }
    Ok(uri)
}

/// Serde helpers mapping strongly-typed config fields to their JSON form.
/// Both directions validate, so an invalid value cannot enter the model
/// through the management API either.
#[cfg(feature = "api")]
mod serde_impl {
    use super::normalize_auth_endpoint;
    use hyper::header::HeaderName;
    use hyper::Uri;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub(super) mod uri {
        use super::*;

        pub fn serialize<S: Serializer>(value: &Uri, serializer: S) -> Result<S::Ok, S::Error> {
            value.to_string().serialize(serializer)
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Uri, D::Error> {
            let s = String::deserialize(deserializer)?;
            let uri = Uri::try_from(s.as_str()).map_err(serde::de::Error::custom)?;
            normalize_auth_endpoint(uri).map_err(serde::de::Error::custom)
        }
    }

    pub(super) mod header_name_list {
        use super::*;

        pub fn serialize<S: Serializer>(
            value: &[HeaderName],
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            let names: Vec<&str> = value.iter().map(|n| n.as_str()).collect();
            names.serialize(serializer)
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Vec<HeaderName>, D::Error> {
            let raw = Vec::<String>::deserialize(deserializer)?;
            raw.into_iter()
                .map(|s| HeaderName::from_bytes(s.as_bytes()).map_err(serde::de::Error::custom))
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_config() -> ForwardAuthConfig {
        ForwardAuthConfig {
            endpoint: "http://auth.internal:8080/verify".parse().unwrap(),
            request_headers: vec![
                HeaderName::from_static("authorization"),
                HeaderName::from_static("cookie"),
            ],
            response_headers: vec![
                HeaderName::from_static("x-user"),
                HeaderName::from_static("x-email"),
            ],
            failure_mode: FailureMode::Open,
            timeout_secs: 10,
        }
    }

    #[test]
    fn test_normalize_auth_endpoint_fills_default_path() {
        let uri = normalize_auth_endpoint("http://auth.internal:8080".parse().unwrap()).unwrap();
        assert_eq!(uri.path(), "/");
        assert_eq!(uri.authority().unwrap().as_str(), "auth.internal:8080");
    }

    #[test]
    fn test_normalize_auth_endpoint_accepts_path_and_query() {
        let uri = normalize_auth_endpoint("https://auth/verify?x=1".parse().unwrap()).unwrap();
        assert_eq!(uri.path_and_query().unwrap().as_str(), "/verify?x=1");
    }

    #[test]
    fn test_normalize_auth_endpoint_rejects_relative() {
        assert!(normalize_auth_endpoint("/verify".parse().unwrap()).is_err());
    }

    #[test]
    fn test_normalize_auth_endpoint_rejects_other_schemes() {
        assert!(normalize_auth_endpoint("ftp://auth/".parse().unwrap()).is_err());
        // `unix://` cannot even be parsed as a `Uri`, which is equally rejected
        // at config-load time by the parser / serde layer.
        assert!("unix:///run/x.sock".parse::<Uri>().is_err());
    }

    #[cfg(feature = "api")]
    #[test]
    fn test_forward_auth_serde_round_trip() {
        let json = serde_json::to_string(&sample_config()).unwrap();
        assert!(
            json.contains("\"to\":\"http://auth.internal:8080/verify\""),
            "{json}"
        );
        assert!(json.contains("\"failure_mode\":\"open\""), "{json}");

        let parsed: ForwardAuthConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.endpoint, sample_config().endpoint);
        assert_eq!(parsed.failure_mode, FailureMode::Open);
        assert_eq!(parsed.timeout_secs, 10);
        assert_eq!(
            parsed.request_headers,
            vec![
                HeaderName::from_static("authorization"),
                HeaderName::from_static("cookie"),
            ]
        );
        assert_eq!(parsed.response_headers.len(), 2);
    }

    #[cfg(feature = "api")]
    #[test]
    fn test_forward_auth_serde_defaults() {
        let parsed: ForwardAuthConfig =
            serde_json::from_str(r#"{"to": "http://auth/verify"}"#).unwrap();
        assert_eq!(parsed.failure_mode, FailureMode::Closed);
        assert_eq!(parsed.timeout_secs, default_auth_timeout_secs());
        assert_eq!(parsed.request_headers, default_request_headers());
        assert!(parsed.response_headers.is_empty());
    }

    #[cfg(feature = "api")]
    #[test]
    fn test_forward_auth_serde_rejects_invalid_values() {
        assert!(
            serde_json::from_str::<ForwardAuthConfig>(r#"{"to": "ftp://auth/verify"}"#).is_err()
        );
        assert!(serde_json::from_str::<ForwardAuthConfig>(r#"{"to": "/verify"}"#).is_err());
        assert!(serde_json::from_str::<ForwardAuthConfig>(
            r#"{"to": "http://auth/verify", "request_headers": ["Bad Header!"]}"#
        )
        .is_err());
        assert!(serde_json::from_str::<ForwardAuthConfig>(
            r#"{"to": "http://auth/verify", "failure_mode": "banana"}"#
        )
        .is_err());
    }
}
