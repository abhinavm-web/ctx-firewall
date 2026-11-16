use axum::{
    body::Body,
    http::{header, Request, StatusCode},
    response::Response,
    routing::post,
    Router,
};
use futures_util::stream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::net::TcpListener;

#[tokio::test]
async fn test_proxy_streaming_and_pruning_e2e() {
    let prefix_verified = Arc::new(AtomicBool::new(false));
    let dedup_verified = Arc::new(AtomicBool::new(false));
    let ansi_stripped_verified = Arc::new(AtomicBool::new(false));
    let log_folded_verified = Arc::new(AtomicBool::new(false));

    let p_v = prefix_verified.clone();
    let d_v = dedup_verified.clone();
    let a_v = ansi_stripped_verified.clone();
    let l_v = log_folded_verified.clone();

    // 1. Mock upstream server
    let mock_app = Router::new().route(
        "/v1/messages",
        post(move |req: Request<Body>| {
            let p_v = p_v.clone();
            let d_v = d_v.clone();
            let a_v = a_v.clone();
            let l_v = l_v.clone();
            async move {
                let bytes = axum::body::to_bytes(req.into_body(), 10 * 1024 * 1024)
                    .await
                    .unwrap();
                let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

                // Invariant 1: Pinned prefix preserved bit-for-bit
                if let Some(sys) = json.get("system").and_then(|s| s.as_str()) {
                    if sys == "You are a senior Rust systems engineer." {
                        p_v.store(true, Ordering::SeqCst);
                    }
                }

                let body_str = String::from_utf8_lossy(&bytes);

                // Invariant 2: Duplicate file read replaced with tombstone
                if body_str.contains("[ctx-firewall: content identical") {
                    d_v.store(true, Ordering::SeqCst);
                }

                // Invariant 3: ANSI sequences stripped
                if !body_str.contains("\x1b[32m") && body_str.contains("test result: ok") {
                    a_v.store(true, Ordering::SeqCst);
                }

                // Invariant 4: Log output folded
                if body_str.contains("folded") && body_str.contains("lines of repetitive build/test output") {
                    l_v.store(true, Ordering::SeqCst);
                }

                // Compliant Anthropic SSE stream response
                let sse_chunks = vec![
                    "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"role\":\"assistant\",\"usage\":{\"input_tokens\":1200,\"cache_read_input_tokens\":1000}}}\n\n",
                    "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello from upstream!\"}}\n\n",
                    "event: message_delta\ndata: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":15}}\n\n",
                    "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
                ];

                let stream = stream::iter(
                    sse_chunks
                        .into_iter()
                        .map(|s| Ok::<_, std::io::Error>(s.as_bytes().to_vec())),
                );
                Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "text/event-stream")
                    .body(Body::from_stream(stream))
                    .unwrap()
            }
        }),
    );

    let mock_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_addr = mock_listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(mock_listener, mock_app).await.unwrap();
    });

    // 2. Start ctx-firewall proxy targeting mock upstream
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let proxy_app = ctx_firewall::proxy::create_router(
        format!("http://{}", mock_addr),
        format!("http://{}", mock_addr),
        tx,
    );
    let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = proxy_listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(proxy_listener, proxy_app).await.unwrap();
    });

    // 3. Client sends payload with:
    //    - Pinned prefix: system prompt
    //    - History Turn 1: user asked to read main.rs, assistant executed read_file(src/main.rs),
    //      user returned content
    //    - Tail Turn 2: assistant repeated read_file(src/main.rs), user returned identical content,
    //      and tool_2 returned ANSI-colored verbose test log
    let file_content = "// Line of source code in main.rs\n".repeat(30);
    let verbose_log = format!(
        "\x1b[32mtest result: ok\x1b[0m. 100 passed; 0 failed;\n{}",
        "test run line ... ok\n".repeat(70)
    );

    let payload = serde_json::json!({
        "model": "claude-3-5-sonnet-20241022",
        "system": "You are a senior Rust systems engineer.",
        "messages": [
            {
                "role": "user",
                "content": [
                    {
                        "type": "text",
                        "text": "Please inspect src/main.rs",
                        "cache_control": { "type": "ephemeral" }
                    }
                ]
            },
            {
                "role": "assistant",
                "content": [
                    {
                        "type": "tool_use",
                        "id": "tool_hist_1",
                        "name": "read_file",
                        "input": { "path": "src/main.rs" }
                    }
                ]
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "tool_hist_1",
                        "content": file_content
                    }
                ]
            },
            {
                "role": "assistant",
                "content": [
                    {
                        "type": "tool_use",
                        "id": "tool_tail_1",
                        "name": "read_file",
                        "input": { "path": "src/main.rs" }
                    },
                    {
                        "type": "tool_use",
                        "id": "tool_tail_2",
                        "name": "run_command",
                        "input": { "command": "cargo test" }
                    }
                ]
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "tool_tail_1",
                        "content": file_content
                    },
                    {
                        "type": "tool_result",
                        "tool_use_id": "tool_tail_2",
                        "content": verbose_log
                    }
                ]
            }
        ],
        "stream": true
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{}/v1/messages", proxy_addr))
        .header("content-type", "application/json")
        .header("x-api-key", "test-key")
        .header("anthropic-version", "2023-06-01")
        .json(&payload)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    assert_eq!(
        resp.headers().get("content-type").unwrap(),
        "text/event-stream"
    );

    let text = resp.text().await.unwrap();
    assert!(text.contains("Hello from upstream!"));

    // Invariants verified by upstream inspector
    assert!(
        prefix_verified.load(Ordering::SeqCst),
        "Pinned prefix was modified!"
    );
    assert!(
        dedup_verified.load(Ordering::SeqCst),
        "Duplicate file read was not deduplicated!"
    );
    assert!(
        ansi_stripped_verified.load(Ordering::SeqCst),
        "ANSI sequences were not stripped!"
    );
    assert!(
        log_folded_verified.load(Ordering::SeqCst),
        "Repetitive test log lines were not folded!"
    );
}

#[tokio::test]
async fn test_proxy_health_check() {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let proxy_app = ctx_firewall::proxy::create_router(
        "https://api.anthropic.com".to_string(),
        "https://api.openai.com".to_string(),
        tx,
    );
    let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = proxy_listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(proxy_listener, proxy_app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://{}/health", proxy_addr))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");
    assert!(body.get("uptime_secs").is_some());
}

#[tokio::test]
async fn test_proxy_openai_streaming_e2e() {
    let mock_app = Router::new().route(
        "/v1/chat/completions",
        post(|_req: Request<Body>| async {
            let sse_chunks = vec![
                "data: {\"choices\":[{\"delta\":{\"content\":\"Hello from OpenAI!\"}}],\"usage\":{\"prompt_tokens\":50,\"completion_tokens\":10}}\n\n",
                "data: [DONE]\n\n",
            ];
            let stream = stream::iter(
                sse_chunks
                    .into_iter()
                    .map(|s| Ok::<_, std::io::Error>(s.as_bytes().to_vec())),
            );
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/event-stream")
                .body(Body::from_stream(stream))
                .unwrap()
        }),
    );

    let mock_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_addr = mock_listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(mock_listener, mock_app).await.unwrap();
    });

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let proxy_app = ctx_firewall::proxy::create_router(
        format!("http://{}", mock_addr),
        format!("http://{}", mock_addr),
        tx,
    );
    let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = proxy_listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(proxy_listener, proxy_app).await.unwrap();
    });

    let payload = serde_json::json!({
        "model": "gpt-4o",
        "messages": [
            { "role": "system", "content": "You are a helpful assistant." },
            { "role": "user", "content": "Hello!" }
        ],
        "stream": true
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{}/v1/chat/completions", proxy_addr))
        .header("content-type", "application/json")
        .header("authorization", "Bearer sk-test")
        .json(&payload)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let text = resp.text().await.unwrap();
    assert!(text.contains("Hello from OpenAI!"));
}
