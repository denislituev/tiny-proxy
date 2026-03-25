//! Benchmark suite for tiny-proxy
//!
//! This benchmark suite measures the performance of various proxy operations:
//! - Proxy instance creation
//! - Configuration parsing
//! - Pattern matching
//! - Directive operations
//! - Header manipulation
//! - URI operations
//!
//! Run with:
//! ```bash
//! cargo bench
//! ```
//!
//! For specific benchmark groups:
//! ```bash
//! cargo bench --bench proxy_bench -- config_parsing
//! cargo bench --bench proxy_bench -- pattern_matching
//! ```

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use std::str::FromStr;
use tiny_proxy::config::{Config, Directive, SiteConfig};
use tiny_proxy::proxy::handler::match_pattern;

/// Benchmark proxy instance creation
fn bench_proxy_creation(c: &mut Criterion) {
    let config = create_simple_config();

    c.bench_function("proxy_creation", |b| {
        b.iter(|| {
            let _ = tiny_proxy::Proxy::new(config.clone());
        })
    });

    // Benchmark with different config sizes
    let mut group = c.benchmark_group("proxy_creation_by_size");

    for (name, config) in [
        ("empty", create_empty_config()),
        ("simple", create_simple_config()),
        ("medium", create_medium_config()),
        ("complex", create_complex_config()),
    ] {
        group.bench_with_input(BenchmarkId::from_parameter(name), &config, |b, config| {
            b.iter(|| tiny_proxy::Proxy::new(config.clone()))
        });
    }

    group.finish();
}

/// Benchmark configuration parsing
fn bench_config_parsing(c: &mut Criterion) {
    // Simple configuration
    let simple_config = r#"
localhost:8080 {
    reverse_proxy backend:9001
}

localhost:8081 {
    reverse_proxy backend:9002
}
"#;

    c.bench_function("config_parsing_simple", |b| {
        b.iter(|| Config::from_str(black_box(simple_config)))
    });

    // Medium configuration
    let medium_config = r#"
localhost:8080 {
    handle_path /api/* {
        header X-Version v1
        reverse_proxy api:8001
    }
    handle_path /static/* {
        reverse_proxy static:8002
    }
    reverse_proxy backend:9000
}

localhost:8081 {
    header X-Proxy tiny-proxy
    uri_replace /old /new
    reverse_proxy backend:9001
}
"#;

    c.bench_function("config_parsing_medium", |b| {
        b.iter(|| Config::from_str(black_box(medium_config)))
    });

    // Complex configuration
    let complex_config = r#"
localhost:8080 {
    handle_path /api/v1/* {
        header X-Version v1
        handle_path /users/* {
            header X-Service users
            reverse_proxy users:8001
        }
        handle_path /orders/* {
            header X-Service orders
            reverse_proxy orders:8002
        }
        reverse_proxy api:8000
    }
    handle_path /api/v2/* {
        header X-Version v2
        reverse_proxy api:8000
    }
    header X-Gateway tiny-proxy
    reverse_proxy fallback:9000
}

localhost:8081 {
    handle_path /health {
        respond 200 "OK"
    }
    reverse_proxy backend:9001
}

localhost:8082 {
    method GET HEAD {
        respond 200 "Cacheable"
    }
    reverse_proxy backend:9002
}
"#;

    c.bench_function("config_parsing_complex", |b| {
        b.iter(|| Config::from_str(black_box(complex_config)))
    });

    // Parse with varying line counts
    let mut group = c.benchmark_group("config_parsing_by_size");

    for (lines, config) in [
        (5, create_test_config(5)),
        (10, create_test_config(10)),
        (20, create_test_config(20)),
        (50, create_test_config(50)),
        (100, create_test_config(100)),
    ] {
        group.bench_with_input(BenchmarkId::from_parameter(lines), &config, |b, config| {
            b.iter(|| Config::from_str(black_box(config)))
        });
    }

    group.finish();
}

/// Benchmark pattern matching with exact paths
fn bench_pattern_matching_exact(c: &mut Criterion) {
    let mut group = c.benchmark_group("pattern_matching_exact");

    let test_cases = [
        ("/api/users", "/api/users"),
        ("/api/v1/posts", "/api/v1/posts"),
        ("/health", "/health"),
        ("/static/image.png", "/static/image.png"),
        ("/api/v2/orders/123", "/api/v2/orders/123"),
        ("/events/stream", "/events/stream"),
    ];

    for (pattern, path) in test_cases.iter() {
        group.bench_with_input(BenchmarkId::new("match", path), path, |b, path| {
            b.iter(|| match_pattern(black_box(pattern), black_box(path)))
        });
    }

    // Benchmark matching vs non-matching
    group.bench_function("match_success", |b| {
        b.iter(|| match_pattern(black_box("/api/users"), black_box("/api/users")))
    });

    group.bench_function("match_failure", |b| {
        b.iter(|| match_pattern(black_box("/api/users"), black_box("/api/orders")))
    });

    group.finish();
}

/// Benchmark pattern matching with wildcards
fn bench_pattern_matching_wildcard(c: &mut Criterion) {
    let mut group = c.benchmark_group("pattern_matching_wildcard");

    let test_cases = [
        ("/api/*", "/api/users/123"),
        ("/api/*", "/api/posts/456"),
        ("/api/v1/*", "/api/v1/users"),
        ("/api/v1/*", "/api/v1/posts/789"),
        ("/static/*", "/static/css/style.css"),
        ("/static/*", "/static/js/app.js"),
        ("/events/*", "/events/stream"),
        ("/events/*", "/events/push"),
    ];

    for (pattern, path) in test_cases.iter() {
        group.bench_with_input(BenchmarkId::new("match", path), path, |b, path| {
            b.iter(|| match_pattern(black_box(pattern), black_box(path)))
        });
    }

    group.finish();

    // Wildcard pattern length impact
    let mut pattern_group = c.benchmark_group("wildcard_pattern_depth");

    for (depth, config) in [
        (1, "/api/*"),
        (2, "/api/v1/*"),
        (3, "/api/v1/users/*"),
        (4, "/api/v1/users/123/*"),
    ] {
        pattern_group.bench_with_input(BenchmarkId::from_parameter(depth), config, |b, pattern| {
            b.iter(|| match_pattern(black_box(pattern), black_box("/api/v1/users/123/data")))
        });
    }

    pattern_group.finish();
}

/// Benchmark URI string operations
fn bench_uri_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("uri_operations");

    // Benchmark URI parsing
    let uris = [
        "http://localhost:8080/api/users/123",
        "https://example.com:443/path/to/resource?query=value",
        "http://backend:9001/api/v1/posts",
        "https://api.service.io/v2/orders/456/items/789",
        "http://localhost:8080/very/long/path/with/many/segments/123/456/789",
    ];

    for uri_str in uris.iter() {
        group.bench_with_input(BenchmarkId::new("parse", uri_str), uri_str, |b, s| {
            b.iter(|| s.parse::<hyper::Uri>())
        });
    }

    // Benchmark string replacement (uri_replace logic)
    let replacement_cases = [
        ("/old/path", "/new", "/old/path"),
        ("/api/v1", "/api/v2", "/api/v1/users"),
        ("/old-prefix", "/new-prefix", "/old-prefix/resource"),
    ];

    for (find, replace, path) in replacement_cases.iter() {
        group.bench_with_input(BenchmarkId::new("replace", path), path, |b, path| {
            b.iter(|| path.replace(black_box(find), black_box(replace)))
        });
    }

    group.finish();
}

/// Benchmark header operations
fn bench_header_operations(c: &mut Criterion) {
    use hyper::header::HeaderValue;
    use hyper::HeaderMap;

    let mut group = c.benchmark_group("header_operations");

    // Create a header map with multiple headers
    let mut headers = HeaderMap::new();
    headers.insert("Authorization", HeaderValue::from_static("Bearer token123"));
    headers.insert("X-Request-ID", HeaderValue::from_static("abc-123"));
    headers.insert("X-Forwarded-For", HeaderValue::from_static("192.168.1.1"));
    headers.insert("User-Agent", HeaderValue::from_static("tiny-proxy/0.1"));

    group.bench_function("header_get", |b| {
        b.iter(|| black_box(&headers).get("Authorization"))
    });

    group.bench_function("header_get_multiple", |b| {
        b.iter(|| {
            black_box(&headers).get("Authorization");
            black_box(&headers).get("X-Request-ID");
            black_box(&headers).get("X-Forwarded-For");
        })
    });

    group.bench_function("header_insert", |b| {
        let mut map = HeaderMap::new();
        b.iter(|| {
            black_box(&mut map).insert("X-Test", HeaderValue::from_static("test"));
        })
    });

    // Header parsing for placeholders
    let placeholder_patterns = [
        "Bearer {header.Authorization}",
        "X-Request-ID: {header.X-Request-ID}",
        "{header.Authorization}, {header.X-Request-ID}",
        "User: {header.User-Agent}, IP: {header.X-Forwarded-For}",
    ];

    for pattern in placeholder_patterns.iter() {
        group.bench_with_input(
            BenchmarkId::new("placeholder_parse", pattern),
            pattern,
            |b, p| b.iter(|| p.replace("{header.Authorization}", "Bearer token123")),
        );
    }

    group.finish();
}

/// Benchmark Directive struct operations
fn bench_directive_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("directive_operations");

    // Benchmark directive cloning
    let directives = vec![
        Directive::Header {
            name: "X-Custom".to_string(),
            value: "value".to_string(),
        },
        Directive::ReverseProxy {
            to: "http://backend:9001".to_string(),
        },
        Directive::UriReplace {
            find: "/old".to_string(),
            replace: "/new".to_string(),
        },
    ];

    group.bench_function("directive_clone_small", |b| {
        b.iter(|| black_box(&directives).clone())
    });

    // Larger directive set
    let large_directives = create_large_directive_list(50);

    group.bench_function("directive_clone_large", |b| {
        b.iter(|| black_box(&large_directives).clone())
    });

    group.finish();
}

/// Benchmark config operations
fn bench_config_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("config_operations");

    // Config cloning
    let config = create_complex_config();

    group.bench_function("config_clone", |b| b.iter(|| black_box(&config).clone()));

    // Config site lookup
    group.bench_function("config_site_lookup", |b| {
        b.iter(|| black_box(&config).sites.get("localhost:8080"))
    });

    // Config site lookup with multiple sites
    let multi_site_config = create_multi_site_config(10);

    group.bench_with_input(
        BenchmarkId::from_parameter("10_sites"),
        &multi_site_config,
        |b, config| b.iter(|| black_box(config).sites.get("site5")),
    );

    group.finish();
}

/// Benchmark HashMap operations (Config.sites backend)
fn bench_hashmap_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("hashmap_operations");

    // Create config with varying number of sites
    for size in [1, 5, 10, 20, 50, 100] {
        let config = create_multi_site_config(size);

        group.bench_with_input(BenchmarkId::new("lookup", size), &config, |b, config| {
            let key = format!("site{}", size / 2);
            b.iter(|| black_box(config).sites.get(black_box(&key)))
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_proxy_creation,
    bench_config_parsing,
    bench_pattern_matching_exact,
    bench_pattern_matching_wildcard,
    bench_uri_operations,
    bench_header_operations,
    bench_directive_operations,
    bench_config_operations,
    bench_hashmap_operations
);

criterion_main!(benches);

// ===== Helper Functions =====

fn create_empty_config() -> Config {
    Config {
        sites: std::collections::HashMap::new(),
    }
}

fn create_simple_config() -> Config {
    Config {
        sites: std::collections::HashMap::from([(
            "localhost:8080".to_string(),
            SiteConfig {
                address: "localhost:8080".to_string(),
                directives: vec![
                    Directive::Header {
                        name: "X-Forwarded-For".to_string(),
                        value: "{remote_host}".to_string(),
                    },
                    Directive::ReverseProxy {
                        to: "http://127.0.0.1:9001".to_string(),
                    },
                ],
            },
        )]),
    }
}

fn create_medium_config() -> Config {
    Config {
        sites: std::collections::HashMap::from([
            (
                "localhost:8080".to_string(),
                SiteConfig {
                    address: "localhost:8080".to_string(),
                    directives: vec![
                        Directive::Header {
                            name: "X-Proxy".to_string(),
                            value: "tiny-proxy".to_string(),
                        },
                        Directive::UriReplace {
                            find: "/old".to_string(),
                            replace: "/new".to_string(),
                        },
                        Directive::Header {
                            name: "X-Custom".to_string(),
                            value: "value".to_string(),
                        },
                        Directive::Header {
                            name: "X-Another".to_string(),
                            value: "value2".to_string(),
                        },
                        Directive::ReverseProxy {
                            to: "http://backend:9001".to_string(),
                        },
                    ],
                },
            ),
            (
                "localhost:8081".to_string(),
                SiteConfig {
                    address: "localhost:8081".to_string(),
                    directives: vec![Directive::ReverseProxy {
                        to: "http://backend:9002".to_string(),
                    }],
                },
            ),
        ]),
    }
}

fn create_complex_config() -> Config {
    Config {
        sites: std::collections::HashMap::from([(
            "localhost:8080".to_string(),
            SiteConfig {
                address: "localhost:8080".to_string(),
                directives: vec![
                    Directive::HandlePath {
                        pattern: "/api/*".to_string(),
                        directives: vec![
                            Directive::Header {
                                name: "X-API-Version".to_string(),
                                value: "v1".to_string(),
                            },
                            Directive::HandlePath {
                                pattern: "/users/*".to_string(),
                                directives: vec![
                                    Directive::Header {
                                        name: "X-Service".to_string(),
                                        value: "user-service".to_string(),
                                    },
                                    Directive::ReverseProxy {
                                        to: "http://user-service:8001".to_string(),
                                    },
                                ],
                            },
                            Directive::HandlePath {
                                pattern: "/orders/*".to_string(),
                                directives: vec![
                                    Directive::Header {
                                        name: "X-Service".to_string(),
                                        value: "order-service".to_string(),
                                    },
                                    Directive::ReverseProxy {
                                        to: "http://order-service:8002".to_string(),
                                    },
                                ],
                            },
                            Directive::Header {
                                name: "X-Service".to_string(),
                                value: "api-service".to_string(),
                            },
                            Directive::Header {
                                name: "X-Cache-Control".to_string(),
                                value: "no-cache".to_string(),
                            },
                            Directive::UriReplace {
                                find: "/api".to_string(),
                                replace: "".to_string(),
                            },
                            Directive::ReverseProxy {
                                to: "http://api-service:8000".to_string(),
                            },
                        ],
                    },
                    Directive::HandlePath {
                        pattern: "/health".to_string(),
                        directives: vec![Directive::Respond {
                            status: 200,
                            body: "OK".to_string(),
                        }],
                    },
                ],
            },
        )]),
    }
}

fn create_multi_site_config(count: usize) -> Config {
    let mut sites = std::collections::HashMap::new();

    for i in 0..count {
        let site_name = format!("site{}", i);
        let address = format!("localhost:{}", 8000 + i);

        sites.insert(
            site_name.clone(),
            SiteConfig {
                address,
                directives: vec![
                    Directive::Header {
                        name: "X-Site-ID".to_string(),
                        value: i.to_string(),
                    },
                    Directive::ReverseProxy {
                        to: format!("http://backend:{}", 9000 + i),
                    },
                ],
            },
        );
    }

    Config { sites }
}

fn create_large_directive_list(count: usize) -> Vec<Directive> {
    let mut directives = Vec::with_capacity(count);

    for i in 0..count {
        if i % 3 == 0 {
            directives.push(Directive::Header {
                name: format!("X-Header-{}", i),
                value: format!("value-{}", i),
            });
        } else if i % 3 == 1 {
            directives.push(Directive::UriReplace {
                find: format!("/old{}", i),
                replace: format!("/new{}", i),
            });
        } else {
            directives.push(Directive::Method {
                methods: vec!["GET".to_string(), "POST".to_string()],
                directives: vec![],
            });
        }
    }

    directives
}

fn create_test_config(lines: usize) -> String {
    let mut config = String::new();

    for i in 0..lines {
        config.push_str(&format!(
            "localhost:{} {{\n    reverse_proxy backend:{}\n}}\n",
            8000 + i,
            9000 + i
        ));
    }

    config
}
