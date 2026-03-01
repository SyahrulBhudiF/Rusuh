//! SSE stream forwarding — handles chunked transfer encoding, partial line buffering,
//! and converts upstream SSE events into OpenAI-compatible SSE format.

use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::{json, Value};

use crate::error::AppError;
use crate::providers::BoxStream;

/// Wrap a raw byte stream (possibly chunked) into a properly buffered SSE stream
/// that handles partial lines across chunk boundaries.
///
/// `transform_fn` is called for each complete SSE `data:` payload (excluding `[DONE]`).
/// It should return `Some(sse_line)` to emit or `None` to skip.
pub fn buffered_sse_stream<F>(
    upstream: impl Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
    transform_fn: F,
) -> BoxStream
where
    F: Fn(&str) -> Option<String> + Send + 'static,
{
    let stream = async_stream::stream! {
        let mut buf = String::new();

        futures::pin_mut!(upstream);

        while let Some(chunk_result) = upstream.next().await {
            let chunk = match chunk_result {
                Ok(c) => c,
                Err(e) => {
                    // Emit error as SSE event and stop
                    let err_event = format!(
                        "data: {}\n\n",
                        serde_json::to_string(&json!({
                            "error": { "message": format!("stream read error: {e}"), "type": "stream_error" }
                        })).unwrap_or_default()
                    );
                    yield Ok(Bytes::from(err_event));
                    break;
                }
            };

            buf.push_str(&String::from_utf8_lossy(&chunk));

            // Process complete lines (SSE lines end with \n)
            while let Some(newline_pos) = buf.find('\n') {
                let line = buf[..newline_pos].trim_end_matches('\r').to_string();
                buf = buf[newline_pos + 1..].to_string();

                let line = line.trim();

                // Empty line = SSE event boundary, skip
                if line.is_empty() {
                    continue;
                }

                // Only process "data: ..." lines
                if !line.starts_with("data: ") && !line.starts_with("data:") {
                    // Could be event:, id:, retry: — skip for now
                    continue;
                }

                let data_str = line.strip_prefix("data: ")
                    .or_else(|| line.strip_prefix("data:"))
                    .unwrap_or("")
                    .trim();

                if data_str == "[DONE]" {
                    yield Ok(Bytes::from("data: [DONE]\n\n"));
                    continue;
                }

                if data_str.is_empty() {
                    continue;
                }

                if let Some(sse_line) = transform_fn(data_str) {
                    yield Ok(Bytes::from(format!("data: {sse_line}\n\n")));
                }
            }
        }

        // If there's leftover in buffer, try to process it
        let remaining = buf.trim();
        if !remaining.is_empty() {
            if let Some(data_str) = remaining.strip_prefix("data: ")
                .or_else(|| remaining.strip_prefix("data:"))
            {
                let data_str = data_str.trim();
                if data_str == "[DONE]" {
                    yield Ok(Bytes::from("data: [DONE]\n\n"));
                } else if !data_str.is_empty() {
                    if let Some(sse_line) = transform_fn(data_str) {
                        yield Ok(Bytes::from(format!("data: {sse_line}\n\n")));
                    }
                }
            }
        }
    };

    Box::pin(stream)
}

/// Build an Antigravity→OpenAI SSE transform function.
///
/// Each Antigravity SSE `data:` payload is a JSON object with Gemini-like structure.
/// This converts it into an OpenAI `chat.completion.chunk` JSON string.
pub fn antigravity_to_openai_transform(
    id: String,
    model: String,
    created: i64,
) -> impl Fn(&str) -> Option<String> + Send + 'static {
    move |data_str: &str| {
        let data: Value = serde_json::from_str(data_str).ok()?;

        let response = data.get("response").unwrap_or(&data);
        let candidate = &response["candidates"][0];
        let parts = candidate["content"]["parts"].as_array();

        let content = parts
            .and_then(|p| p.iter().find_map(|part| part["text"].as_str()))
            .unwrap_or("");

        let finish = candidate["finishReason"]
            .as_str()
            .and_then(|r| match r {
                "STOP" => Some("stop"),
                "MAX_TOKENS" | "MAX_OUTPUT_TOKENS" => Some("length"),
                "TOOL_CALL" => Some("tool_calls"),
                _ => None,
            });

        // Build tool_calls delta if present
        let tool_calls: Option<Vec<Value>> = parts.and_then(|p| {
            let calls: Vec<Value> = p
                .iter()
                .filter_map(|part| {
                    let fc = part.get("functionCall")?;
                    Some(json!({
                        "index": 0,
                        "id": format!("call_{}", uuid::Uuid::new_v4()),
                        "type": "function",
                        "function": {
                            "name": fc.get("name").and_then(|n| n.as_str()).unwrap_or(""),
                            "arguments": fc.get("args").map(|a| a.to_string()).unwrap_or_default(),
                        }
                    }))
                })
                .collect();
            if calls.is_empty() { None } else { Some(calls) }
        });

        // Skip chunks with no content and no tool calls and no finish reason
        if content.is_empty() && tool_calls.is_none() && finish.is_none() {
            return None;
        }

        let mut delta = json!({});
        if !content.is_empty() {
            delta["content"] = json!(content);
        }
        if let Some(tc) = tool_calls {
            delta["tool_calls"] = json!(tc);
        }

        let chunk = json!({
            "id": id,
            "object": "chat.completion.chunk",
            "created": created,
            "model": model,
            "choices": [{
                "index": 0,
                "delta": delta,
                "finish_reason": finish,
            }]
        });

        serde_json::to_string(&chunk).ok()
    }
}

/// Build an SSE Response from a BoxStream with proper headers.
pub fn sse_response(stream: BoxStream) -> axum::response::Response {
    let body = axum::body::Body::from_stream(stream);
    axum::response::Response::builder()
        .header("Content-Type", "text/event-stream")
        .header("Cache-Control", "no-cache")
        .header("Connection", "keep-alive")
        .header("X-Accel-Buffering", "no")
        .body(body)
        .expect("build sse response")
}

/// Passthrough SSE stream — forwards raw SSE bytes without transformation.
/// Useful for providers that already emit OpenAI-compatible SSE.
pub fn passthrough_sse_stream(
    upstream: impl Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
) -> BoxStream {
    let stream = upstream.map(|chunk| {
        chunk.map_err(|e| AppError::Upstream(format!("stream read: {e}")))
    });
    Box::pin(stream)
}

