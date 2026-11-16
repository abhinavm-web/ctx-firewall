use axum::http::{HeaderMap, HeaderName, HeaderValue};

pub fn is_hop_by_hop(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "host"
            | "content-length"
    )
}

pub fn build_forward_headers(
    incoming_headers: &HeaderMap,
    target_host: &str,
    pruned_body_len: usize,
) -> HeaderMap {
    let mut out = HeaderMap::new();
    for (k, v) in incoming_headers.iter() {
        if !is_hop_by_hop(k.as_str()) {
            out.insert(k.clone(), v.clone());
        }
    }
    if let Ok(host_val) = HeaderValue::from_str(target_host) {
        out.insert(HeaderName::from_static("host"), host_val);
    }
    out.insert(
        HeaderName::from_static("content-length"),
        HeaderValue::from(pruned_body_len),
    );
    out
}

pub fn build_downstream_headers(upstream_headers: &HeaderMap) -> HeaderMap {
    let mut out = HeaderMap::new();
    for (k, v) in upstream_headers.iter() {
        if !is_hop_by_hop(k.as_str()) {
            out.insert(k.clone(), v.clone());
        }
    }
    // Ensure CORS headers for browser-based dev agents
    out.insert(
        HeaderName::from_static("access-control-allow-origin"),
        HeaderValue::from_static("*"),
    );
    out.insert(
        HeaderName::from_static("access-control-allow-headers"),
        HeaderValue::from_static("*"),
    );
    out.insert(
        HeaderName::from_static("access-control-allow-methods"),
        HeaderValue::from_static("POST, GET, OPTIONS"),
    );
    out
}
