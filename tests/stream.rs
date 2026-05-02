use bytes::Bytes;
use futures::stream::{self, StreamExt};
use rusuh::proxy::stream::{
    antigravity_to_openai_transform, buffered_sse_stream, passthrough_sse_stream,
};

/// Collect all bytes from a BoxStream.
async fn collect_stream(s: rusuh::providers::BoxStream) -> String {
    let mut out = Vec::new();
    futures::pin_mut!(s);
    while let Some(chunk) = s.next().await {
        out.extend_from_slice(&chunk.unwrap());
    }
    String::from_utf8(out).unwrap()
}

/// Fake upstream from raw string chunks.
fn fake_upstream(
    chunks: Vec<&str>,
) -> impl futures::Stream<Item = Result<Bytes, reqwest::Error>> + Send {
    let owned: Vec<Bytes> = chunks
        .into_iter()
        .map(|s| Bytes::from(s.to_string()))
        .collect();
    stream::iter(owned.into_iter().map(Ok))
}

#[tokio::test]
async fn complete_sse_lines() {
    let upstream = fake_upstream(vec!["data: {\"text\": \"hello\"}\n\n", "data: [DONE]\n\n"]);

    let s = buffered_sse_stream(upstream, |data| Some(data.to_string()));
    let result = collect_stream(s).await;

    assert!(result.contains("data: {\"text\": \"hello\"}"));
    assert!(result.contains("data: [DONE]"));
}

#[tokio::test]
async fn partial_lines_across_chunks() {
    let upstream = fake_upstream(vec!["data: hel", "lo\n\n"]);

    let s = buffered_sse_stream(upstream, |data| Some(data.to_string()));
    let result = collect_stream(s).await;

    assert!(result.contains("data: hello\n"), "got: {result}");
}

#[tokio::test]
async fn multiple_events_in_one_chunk() {
    let upstream = fake_upstream(vec!["data: first\ndata: second\ndata: [DONE]\n"]);

    let s = buffered_sse_stream(upstream, |data| Some(data.to_string()));
    let result = collect_stream(s).await;

    assert!(result.contains("data: first\n"));
    assert!(result.contains("data: second\n"));
    assert!(result.contains("data: [DONE]"));
}

#[tokio::test]
async fn transform_filters_events() {
    let upstream = fake_upstream(vec!["data: keep\n\ndata: skip\n\ndata: keep2\n\n"]);

    let s = buffered_sse_stream(upstream, |data| {
        if data.contains("skip") {
            None
        } else {
            Some(data.to_string())
        }
    });
    let result = collect_stream(s).await;

    assert!(result.contains("keep"));
    assert!(result.contains("keep2"));
    assert!(!result.contains("skip"));
}

#[tokio::test]
async fn antigravity_transform_basic() {
    let transform = antigravity_to_openai_transform("test-id".into(), "test-model".into(), 1000);

    let input = r#"{"response":{"candidates":[{"content":{"parts":[{"text":"hi"}]},"finishReason":"STOP"}]}}"#;
    let result = transform(input).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert_eq!(parsed["id"], "test-id");
    assert_eq!(parsed["object"], "chat.completion.chunk");
    assert_eq!(parsed["model"], "test-model");
    assert_eq!(parsed["created"], 1000);
    assert_eq!(parsed["choices"][0]["delta"]["content"], "hi");
    assert_eq!(parsed["choices"][0]["finish_reason"], "stop");
}

#[tokio::test]
async fn antigravity_transform_max_tokens() {
    let transform = antigravity_to_openai_transform("id".into(), "m".into(), 0);

    let input = r#"{"response":{"candidates":[{"content":{"parts":[{"text":"x"}]},"finishReason":"MAX_TOKENS"}]}}"#;
    let result = transform(input).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert_eq!(parsed["choices"][0]["finish_reason"], "length");
}

#[tokio::test]
async fn antigravity_transform_skips_empty() {
    let transform = antigravity_to_openai_transform("id".into(), "m".into(), 0);

    let input = r#"{"response":{"candidates":[{"content":{"parts":[]}}]}}"#;
    assert!(transform(input).is_none());
}

#[tokio::test]
async fn antigravity_transform_tool_call() {
    let transform = antigravity_to_openai_transform("id".into(), "m".into(), 0);

    let input = r#"{"response":{"candidates":[{"content":{"parts":[{"functionCall":{"name":"get_weather","args":{"city":"NYC"}}}]},"finishReason":"TOOL_CALL"}]}}"#;
    let result = transform(input).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();

    assert_eq!(parsed["choices"][0]["finish_reason"], "tool_calls");
    let tc = &parsed["choices"][0]["delta"]["tool_calls"][0];
    assert_eq!(tc["function"]["name"], "get_weather");
}

#[tokio::test]
async fn done_sentinel_forwarded() {
    let upstream = fake_upstream(vec!["data: [DONE]\n"]);
    let s = buffered_sse_stream(upstream, |_| unreachable!());
    let result = collect_stream(s).await;
    assert_eq!(result.trim(), "data: [DONE]");
}

#[tokio::test]
async fn non_data_lines_ignored() {
    let upstream = fake_upstream(vec!["event: ping\nid: 1\ndata: hello\n\n"]);
    let s = buffered_sse_stream(upstream, |data| Some(data.to_string()));
    let result = collect_stream(s).await;

    assert!(result.contains("data: hello"));
    assert!(!result.contains("event:"));
    assert!(!result.contains("id:"));
}

#[tokio::test]
async fn passthrough_forwards_raw() {
    let upstream = fake_upstream(vec!["data: raw\n\n"]);
    let s = passthrough_sse_stream(upstream);
    let result = collect_stream(s).await;
    assert_eq!(result, "data: raw\n\n");
}

#[tokio::test]
async fn sse_response_has_correct_headers() {
    use rusuh::proxy::stream::sse_response;

    let upstream = fake_upstream(vec!["data: test\n\n"]);
    let s = buffered_sse_stream(upstream, |d| Some(d.to_string()));
    let resp = sse_response(s);

    assert_eq!(
        resp.headers().get("content-type").unwrap(),
        "text/event-stream"
    );
    assert_eq!(resp.headers().get("cache-control").unwrap(), "no-cache");
    assert_eq!(resp.headers().get("x-accel-buffering").unwrap(), "no");
}
