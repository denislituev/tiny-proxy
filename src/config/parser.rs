use crate::config::{Config, Directive, SiteConfig};
use crate::error::ProxyError;
use std::collections::HashMap;

#[derive(Debug)]
struct PendingBlock {
    directive_type: String, // "handle_path", "method", etc.
    args: Vec<String>,      // Arguments, e.g., ["/api/*"] for handle_path
}

impl Config {
    pub fn from_file(path: &str) -> Result<Self, ProxyError> {
        let content = std::fs::read_to_string(path)?;
        Self::from_str(&content)
    }

    pub fn from_str(content: &str) -> Result<Self, ProxyError> {
        let mut sites = HashMap::new();
        let mut current_site_address: Option<String> = None;

        // Stack for storing directives. Each vector element is a list of directives for the current nesting level.
        // Initially we have one level - the site level.
        let mut directive_stack: Vec<Vec<Directive>> = vec![vec![]];

        // Stack for storing information about the blocks we are currently parsing
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

                // If we are at the top level, this could be the start of a site block
                if directive_stack.len() == 1 && current_site_address.is_none() {
                    current_site_address = Some(parts[0].to_string());
                    continue;
                }

                // Otherwise this is the start of a nested block (handle_path, method, etc.)
                let directive_type = parts[0].to_string();
                let args = parts[1..].iter().map(|s| s.to_string()).collect();

                block_stack.push(PendingBlock {
                    directive_type,
                    args,
                });
                directive_stack.push(vec![]); // Add a new level for nested directives
                continue;
            }

            // 2. Handle closing brace
            if line == "}" {
                if directive_stack.len() > 1 {
                    let finished_directives = directive_stack.pop().unwrap();
                    let block_info = block_stack.pop().unwrap();

                    let completed_directive = match block_info.directive_type.as_str() {
                        "handle_path" => {
                            let pattern = block_info.args.get(0).cloned().unwrap_or_default();
                            Directive::HandlePath {
                                pattern,
                                directives: finished_directives,
                            }
                        }
                        "method" => Directive::Method {
                            methods: block_info.args,
                            directives: finished_directives,
                        },
                        _ => {
                            return Err(ProxyError::Parse(format!(
                                "Unknown block type: {}",
                                block_info.directive_type
                            )))
                        }
                    };

                    // Add the assembled directive to the level above
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
                        directive_stack.push(vec![]); // Prepare vector for the next site
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

            let directive = match directive_name {
                "reverse_proxy" => {
                    let to = args.get(0).cloned().ok_or_else(|| {
                        ProxyError::Parse("Missing backend URL for reverse_proxy".to_string())
                    })?;
                    Directive::ReverseProxy { to: to.to_string() }
                }
                "uri_replace" => {
                    let find = args.get(0).cloned().ok_or_else(|| {
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
                    let name = args.get(0).cloned().ok_or_else(|| {
                        ProxyError::Parse("Missing 'name' arg for header".to_string())
                    })?;
                    let value = args.get(1).cloned().ok_or_else(|| {
                        ProxyError::Parse("Missing 'value' arg for header".to_string())
                    })?;
                    Directive::Header {
                        name: name.to_string(),
                        value: value.to_string(),
                    }
                }
                "respond" => {
                    let status = args.get(0).and_then(|s| s.parse().ok()).ok_or_else(|| {
                        ProxyError::Parse("Invalid status for respond".to_string())
                    })?;
                    let body = args.get(1).cloned().unwrap_or_default();
                    Directive::Respond {
                        status,
                        body: body.to_string(),
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

            // Add the directive to the current nesting level
            directive_stack.last_mut().unwrap().push(directive);
        }

        Ok(Config { sites })
    }
}
