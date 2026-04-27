use crate::config::{Config, Directive, SiteConfig};
use crate::error::ProxyError;
use std::collections::HashMap;
use std::str::FromStr;

#[derive(Debug)]
struct PendingBlock {
    directive_type: String,
    args: Vec<String>,
    // Timeout settings for reverse_proxy blocks (in seconds)
    connect_timeout: Option<u64>,
    read_timeout: Option<u64>,
}

/// Parse a human-readable duration string into seconds.
///
/// Supported formats:
/// - Plain number: `"30"` → 30 seconds
/// - Seconds: `"30s"` → 30
/// - Minutes: `"5m"` → 300
/// - Hours: `"2h"` → 7200
/// - Days: `"1d"` → 86400
fn parse_duration(s: &str) -> Result<u64, ProxyError> {
    let s = s.trim();
    if s.is_empty() {
        return Err(ProxyError::Parse("Empty duration value".to_string()));
    }

    // Try plain number first (seconds)
    if let Ok(secs) = s.parse::<u64>() {
        return Ok(secs);
    }

    // Parse with suffix
    let (num_part, multiplier) = if let Some(n) = s.strip_suffix('s') {
        (n, 1u64)
    } else if let Some(n) = s.strip_suffix('m') {
        (n, 60u64)
    } else if let Some(n) = s.strip_suffix('h') {
        (n, 3600u64)
    } else if let Some(n) = s.strip_suffix('d') {
        (n, 86400u64)
    } else {
        return Err(ProxyError::Parse(format!(
            "Invalid duration '{}'. Use a plain number or Ns/Nm/Nh/Nd",
            s
        )));
    };

    let value: u64 = num_part
        .parse()
        .map_err(|_| ProxyError::Parse(format!("Invalid numeric value in duration: '{}'", s)))?;

    Ok(value * multiplier)
}

impl Config {
    pub fn from_file(path: &str) -> Result<Self, ProxyError> {
        let content = std::fs::read_to_string(path)?;
        content.parse()
    }
}

impl FromStr for Config {
    type Err = ProxyError;

    fn from_str(content: &str) -> Result<Self, Self::Err> {
        let mut sites = HashMap::new();
        let mut current_site_address: Option<String> = None;

        let mut directive_stack: Vec<Vec<Directive>> = vec![vec![]];
        let mut block_stack: Vec<PendingBlock> = vec![];

        for (line_num, raw_line) in content.lines().enumerate() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // 1. Handle opening brace
            if line.ends_with('{') {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.is_empty() {
                    continue;
                }

                // Top-level site block
                if directive_stack.len() == 1 && current_site_address.is_none() {
                    current_site_address = Some(parts[0].to_string());
                    continue;
                }

                // Nested block (handle_path, method, reverse_proxy, etc.)
                let directive_type = parts[0].to_string();
                // Filter out trailing "{" from args
                let args = parts[1..]
                    .iter()
                    .filter(|s| **s != "{")
                    .map(|s| s.to_string())
                    .collect();

                block_stack.push(PendingBlock {
                    directive_type,
                    args,
                    connect_timeout: None,
                    read_timeout: None,
                });
                directive_stack.push(vec![]);
                continue;
            }

            // 2. Handle closing brace
            if line == "}" {
                if directive_stack.len() > 1 {
                    let finished_directives = directive_stack.pop().unwrap();
                    let block_info = block_stack.pop().unwrap();

                    let completed_directive = match block_info.directive_type.as_str() {
                        "handle_path" => {
                            let pattern = block_info.args.first().cloned().unwrap_or_default();
                            Directive::HandlePath {
                                pattern,
                                directives: finished_directives,
                            }
                        }
                        "method" => Directive::Method {
                            methods: block_info.args,
                            directives: finished_directives,
                        },
                        "reverse_proxy" => {
                            let to = block_info.args.first().cloned().unwrap_or_default();
                            Directive::ReverseProxy {
                                to,
                                connect_timeout: block_info.connect_timeout,
                                read_timeout: block_info.read_timeout,
                            }
                        }
                        _ => {
                            return Err(ProxyError::Parse(format!(
                                "Unknown block type: {}",
                                block_info.directive_type
                            )))
                        }
                    };

                    directive_stack
                        .last_mut()
                        .unwrap()
                        .push(completed_directive);
                } else {
                    // Site block closed
                    if let Some(address) = current_site_address.take() {
                        let site_directives = directive_stack.pop().unwrap();
                        sites.insert(
                            address.clone(),
                            SiteConfig {
                                address,
                                directives: site_directives,
                            },
                        );
                        directive_stack.push(vec![]);
                    }
                }
                continue;
            }

            // 3. Handle simple directives (single line)
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }

            let directive_name = parts[0];
            let args = parts[1..].to_vec();

            // Special handling: timeout settings inside a reverse_proxy block
            if let Some(block) = block_stack.last_mut() {
                if block.directive_type == "reverse_proxy" {
                    match directive_name {
                        "connect_timeout" => {
                            let raw = args.first().cloned().ok_or_else(|| {
                                ProxyError::Parse("Missing value for connect_timeout".to_string())
                            })?;
                            block.connect_timeout = Some(parse_duration(raw).map_err(|e| {
                                ProxyError::Parse(format!(
                                    "Invalid connect_timeout on line {}: {}",
                                    line_num + 1,
                                    e
                                ))
                            })?);
                            continue;
                        }
                        "read_timeout" => {
                            let raw = args.first().cloned().ok_or_else(|| {
                                ProxyError::Parse("Missing value for read_timeout".to_string())
                            })?;
                            block.read_timeout = Some(parse_duration(raw).map_err(|e| {
                                ProxyError::Parse(format!(
                                    "Invalid read_timeout on line {}: {}",
                                    line_num + 1,
                                    e
                                ))
                            })?);
                            continue;
                        }
                        _ => {
                            return Err(ProxyError::Parse(format!(
                                "Unexpected directive '{}' inside reverse_proxy block on line {}. Only connect_timeout and read_timeout are allowed.",
                                directive_name, line_num + 1
                            )));
                        }
                    }
                }
            }

            // Regular directive parsing
            let directive = match directive_name {
                "reverse_proxy" => {
                    let to = args.first().cloned().ok_or_else(|| {
                        ProxyError::Parse("Missing backend URL for reverse_proxy".to_string())
                    })?;
                    Directive::ReverseProxy {
                        to: to.to_string(),
                        connect_timeout: None,
                        read_timeout: None,
                    }
                }
                "uri_replace" => {
                    let find = args.first().cloned().ok_or_else(|| {
                        ProxyError::Parse("Missing 'find' arg for uri_replace".to_string())
                    })?;
                    let replace = args.get(1).cloned().ok_or_else(|| {
                        ProxyError::Parse("Missing 'replace' arg for uri_replace".to_string())
                    })?;
                    Directive::UriReplace {
                        find: find.to_string(),
                        replace: replace.to_string(),
                    }
                }
                "header" => {
                    let raw_name = args.first().cloned().ok_or_else(|| {
                        ProxyError::Parse("Missing 'name' arg for header".to_string())
                    })?;
                    if let Some(name) = raw_name.strip_prefix('-') {
                        if name.is_empty() {
                            return Err(ProxyError::Parse(
                                "Missing header name after '-' for header removal".to_string(),
                            ));
                        }
                        Directive::Header {
                            name: name.to_string(),
                            value: None,
                        }
                    } else {
                        let value = args.get(1).cloned().ok_or_else(|| {
                            ProxyError::Parse("Missing 'value' arg for header".to_string())
                        })?;
                        Directive::Header {
                            name: raw_name.to_string(),
                            value: Some(value.to_string()),
                        }
                    }
                }
                "respond" => {
                    let status = args.first().and_then(|s| s.parse().ok()).ok_or_else(|| {
                        ProxyError::Parse("Invalid status for respond".to_string())
                    })?;
                    let body = args.get(1).cloned().unwrap_or_default();
                    Directive::Respond {
                        status,
                        body: body.to_string(),
                    }
                }
                "strip_prefix" => {
                    let prefix = args.first().cloned().ok_or_else(|| {
                        ProxyError::Parse("Missing 'prefix' arg for strip_prefix".to_string())
                    })?;
                    Directive::StripPrefix {
                        prefix: prefix.to_string(),
                    }
                }
                "redirect" => {
                    let (status, url) = if args.len() >= 2 {
                        let status: u16 = args[0].parse().map_err(|_| {
                            ProxyError::Parse(format!(
                                "Invalid status code for redirect: {}",
                                args[0]
                            ))
                        })?;
                        let url = args[1..].join(" ");
                        (status, url)
                    } else {
                        let url = args.first().cloned().ok_or_else(|| {
                            ProxyError::Parse("Missing 'url' arg for redirect".to_string())
                        })?;
                        (301u16, url.to_string())
                    };
                    Directive::Redirect {
                        status,
                        url: url.to_string(),
                    }
                }
                _ => {
                    return Err(ProxyError::Parse(format!(
                        "Unknown directive '{}' on line {}",
                        directive_name,
                        line_num + 1
                    )))
                }
            };

            directive_stack.last_mut().unwrap().push(directive);
        }

        Ok(Config { sites })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duration_seconds() {
        assert_eq!(parse_duration("30").unwrap(), 30);
        assert_eq!(parse_duration("30s").unwrap(), 30);
    }

    #[test]
    fn test_parse_duration_minutes() {
        assert_eq!(parse_duration("5m").unwrap(), 300);
    }

    #[test]
    fn test_parse_duration_hours() {
        assert_eq!(parse_duration("2h").unwrap(), 7200);
    }

    #[test]
    fn test_parse_duration_days() {
        assert_eq!(parse_duration("1d").unwrap(), 86400);
    }

    #[test]
    fn test_parse_duration_invalid() {
        assert!(parse_duration("").is_err());
        assert!(parse_duration("abc").is_err());
        assert!(parse_duration("10x").is_err());
    }

    #[test]
    fn test_parse_reverse_proxy_simple() {
        let config = "localhost:8080 {\n    reverse_proxy http://backend:9001\n}";
        let result: Config = config.parse().unwrap();
        let site = result.sites.get("localhost:8080").unwrap();

        assert_eq!(site.directives.len(), 1);
        match &site.directives[0] {
            Directive::ReverseProxy {
                to,
                connect_timeout,
                read_timeout,
            } => {
                assert_eq!(to, "http://backend:9001");
                assert_eq!(*connect_timeout, None);
                assert_eq!(*read_timeout, None);
            }
            _ => panic!("Expected ReverseProxy directive"),
        }
    }

    #[test]
    fn test_parse_reverse_proxy_with_timeouts() {
        let config = r#"localhost:8080 {
    reverse_proxy http://backend:9001 {
        connect_timeout 10s
        read_timeout 5m
    }
}"#;
        let result: Config = config.parse().unwrap();
        let site = result.sites.get("localhost:8080").unwrap();

        assert_eq!(site.directives.len(), 1);
        match &site.directives[0] {
            Directive::ReverseProxy {
                to,
                connect_timeout,
                read_timeout,
            } => {
                assert_eq!(to, "http://backend:9001");
                assert_eq!(*connect_timeout, Some(10));
                assert_eq!(*read_timeout, Some(300));
            }
            _ => panic!("Expected ReverseProxy directive"),
        }
    }

    #[test]
    fn test_parse_reverse_proxy_with_connect_timeout_only() {
        let config = r#"localhost:8080 {
    reverse_proxy http://backend:9001 {
        connect_timeout 5s
    }
}"#;
        let result: Config = config.parse().unwrap();
        let site = result.sites.get("localhost:8080").unwrap();

        match &site.directives[0] {
            Directive::ReverseProxy {
                connect_timeout,
                read_timeout,
                ..
            } => {
                assert_eq!(*connect_timeout, Some(5));
                assert_eq!(*read_timeout, None);
            }
            _ => panic!("Expected ReverseProxy directive"),
        }
    }

    #[test]
    fn test_parse_reverse_proxy_block_rejects_unknown_directive() {
        let config = r#"localhost:8080 {
    reverse_proxy http://backend:9001 {
        unknown_setting 42
    }
}"#;
        let result: Result<Config, _> = config.parse();
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("Unexpected directive"), "{}", err_msg);
    }
}
