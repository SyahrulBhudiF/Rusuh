//! AWS Event Stream binary protocol parser for KIRO responses.
//!
//! Implements the AWS Event Stream format used by CodeWhisperer/AmazonQ:
//! - Prelude (12 bytes): total_length + headers_length + prelude_crc
//! - Headers (variable): Contains event type and metadata
//! - Payload (variable): JSON event data
//! - Message CRC (4 bytes): Checksum
//!
//! Reference: CLIProxyAPIPlus internal/runtime/executor/kiro_executor.go

use std::io::{self, Read};

use bytes::Bytes;
use serde_json::Value;
use thiserror::Error;

/// Maximum message size: 10MB
const MAX_EVENT_STREAM_MSG_SIZE: u32 = 10 * 1024 * 1024;

/// Minimum frame size: prelude (12) + message_crc (4)
const MIN_EVENT_STREAM_FRAME_SIZE: u32 = 16;

/// Event Stream error types
#[derive(Debug, Error)]
pub enum EventStreamError {
    #[error("fatal error: {message}")]
    Fatal {
        message: String,
        #[source]
        source: Option<io::Error>,
    },

    #[error("malformed message: {0}")]
    Malformed(String),

    #[error("IO error: {0}")]
    Io(#[from] io::Error),
}

/// Parsed Event Stream message
#[derive(Debug, Clone)]
pub struct EventStreamMessage {
    /// Event type extracted from headers (e.g., "assistantResponseEvent")
    pub event_type: String,
    /// JSON payload
    pub payload: Bytes,
}

/// Event Stream parser
pub struct EventStreamParser<R> {
    reader: R,
}

impl<R: Read> EventStreamParser<R> {
    /// Create a new parser from a reader
    pub fn new(reader: R) -> Self {
        Self { reader }
    }

    /// Read the next message from the stream
    ///
    /// Returns:
    /// - `Ok(Some(message))` - Successfully read a message
    /// - `Ok(None)` - End of stream (EOF)
    /// - `Err(error)` - Parse error
    pub fn read_message(&mut self) -> Result<Option<EventStreamMessage>, EventStreamError> {
        // Read prelude (12 bytes: total_len + headers_len + prelude_crc)
        let mut prelude = [0u8; 12];
        match self.reader.read_exact(&mut prelude) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                return Ok(None); // Normal end of stream
            }
            Err(e) => {
                return Err(EventStreamError::Fatal {
                    message: "failed to read prelude".to_string(),
                    source: Some(e),
                });
            }
        }

        // Parse prelude fields
        let total_length = u32::from_be_bytes([prelude[0], prelude[1], prelude[2], prelude[3]]);
        let headers_length =
            u32::from_be_bytes([prelude[4], prelude[5], prelude[6], prelude[7]]);
        // prelude[8..12] is prelude_crc - we read it but don't validate

        // Boundary check: minimum frame size
        if total_length < MIN_EVENT_STREAM_FRAME_SIZE {
            return Err(EventStreamError::Malformed(format!(
                "invalid message length: {} (minimum is {})",
                total_length, MIN_EVENT_STREAM_FRAME_SIZE
            )));
        }

        // Boundary check: maximum message size
        if total_length > MAX_EVENT_STREAM_MSG_SIZE {
            return Err(EventStreamError::Malformed(format!(
                "message too large: {} bytes (maximum is {})",
                total_length, MAX_EVENT_STREAM_MSG_SIZE
            )));
        }

        // Boundary check: headers length within message bounds
        // Message structure: prelude(12) + headers(headers_length) + payload + message_crc(4)
        // So: headers_length must be <= total_length - 16
        if headers_length > total_length - 16 {
            return Err(EventStreamError::Malformed(format!(
                "headers length {} exceeds message bounds (total: {})",
                headers_length, total_length
            )));
        }

        // Read the rest of the message (total - 12 bytes already read)
        let remaining_size = (total_length - 12) as usize;
        let mut remaining = vec![0u8; remaining_size];
        self.reader.read_exact(&mut remaining)?;

        // Extract event type from headers
        let event_type = if headers_length > 0 && headers_length <= remaining.len() as u32 {
            extract_event_type(&remaining[..headers_length as usize])
        } else {
            String::new()
        };

        // Calculate payload boundaries
        // Payload starts after headers, ends before message_crc (last 4 bytes)
        let payload_start = headers_length as usize;
        let payload_end = remaining.len().saturating_sub(4); // Skip message_crc at end

        // Extract payload
        let payload = if payload_start < payload_end {
            Bytes::copy_from_slice(&remaining[payload_start..payload_end])
        } else {
            Bytes::new() // No payload
        };

        Ok(Some(EventStreamMessage {
            event_type,
            payload,
        }))
    }

    /// Parse all messages from the stream into a vector
    pub fn parse_all(mut self) -> Result<Vec<EventStreamMessage>, EventStreamError> {
        let mut messages = Vec::new();
        while let Some(msg) = self.read_message()? {
            messages.push(msg);
        }
        Ok(messages)
    }
}

/// Extract event type from header bytes
///
/// Headers format:
/// - name_length (1 byte)
/// - name (variable)
/// - value_type (1 byte)
/// - value (variable, depends on type)
///
/// We're looking for the ":event-type" header with string value (type 7)
fn extract_event_type(headers: &[u8]) -> String {
    let mut offset = 0;

    while offset < headers.len() {
        // Read name length
        if offset >= headers.len() {
            break;
        }
        let name_len = headers[offset] as usize;
        offset += 1;

        // Read name
        if offset + name_len > headers.len() {
            break;
        }
        let name = match std::str::from_utf8(&headers[offset..offset + name_len]) {
            Ok(s) => s,
            Err(_) => break,
        };
        offset += name_len;

        // Read value type
        if offset >= headers.len() {
            break;
        }
        let value_type = headers[offset];
        offset += 1;

        // If this is a string type (7) and name is ":event-type", extract the value
        if value_type == 7 && name == ":event-type" {
            // String format: 2-byte length + data
            if offset + 2 > headers.len() {
                break;
            }
            let value_len = u16::from_be_bytes([headers[offset], headers[offset + 1]]) as usize;
            offset += 2;

            if offset + value_len > headers.len() {
                break;
            }
            if let Ok(event_type) = std::str::from_utf8(&headers[offset..offset + value_len]) {
                return event_type.to_string();
            }
            break;
        }

        // Skip this header value
        offset = match skip_header_value(headers, offset, value_type) {
            Some(new_offset) => new_offset,
            None => break,
        };
    }

    String::new()
}

/// Skip a header value based on its type
///
/// Returns the new offset after skipping, or None if invalid
fn skip_header_value(headers: &[u8], offset: usize, value_type: u8) -> Option<usize> {
    match value_type {
        0 | 1 => Some(offset),     // bool true / bool false (no data)
        2 => Some(offset + 1),     // byte
        3 => Some(offset + 2),     // short
        4 => Some(offset + 4),     // int
        5 => Some(offset + 8),     // long
        6 => {
            // byte array (2-byte length + data)
            if offset + 2 > headers.len() {
                return None;
            }
            let value_len = u16::from_be_bytes([headers[offset], headers[offset + 1]]) as usize;
            let new_offset = offset + 2 + value_len;
            if new_offset > headers.len() {
                return None;
            }
            Some(new_offset)
        }
        7 => {
            // string (2-byte length + data)
            if offset + 2 > headers.len() {
                return None;
            }
            let value_len = u16::from_be_bytes([headers[offset], headers[offset + 1]]) as usize;
            let new_offset = offset + 2 + value_len;
            if new_offset > headers.len() {
                return None;
            }
            Some(new_offset)
        }
        8 => Some(offset + 8),  // timestamp
        9 => Some(offset + 16), // uuid
        _ => None,              // unknown type
    }
}

/// Parse JSON payload from an event stream message
pub fn parse_payload(payload: &[u8]) -> Result<Value, serde_json::Error> {
    if payload.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_slice(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_event_type() {
        // Construct a minimal header with ":event-type" = "testEvent"
        let mut headers = Vec::new();

        // Header name: ":event-type" (11 bytes)
        headers.push(11u8); // name length
        headers.extend_from_slice(b":event-type");

        // Value type: 7 (string)
        headers.push(7u8);

        // Value: "testEvent" (9 bytes)
        headers.push(0u8); // length high byte
        headers.push(9u8); // length low byte
        headers.extend_from_slice(b"testEvent");

        let event_type = extract_event_type(&headers);
        assert_eq!(event_type, "testEvent");
    }

    #[test]
    fn test_skip_header_value() {
        let headers = vec![0u8; 20];

        // Test bool (no data)
        assert_eq!(skip_header_value(&headers, 5, 0), Some(5));
        assert_eq!(skip_header_value(&headers, 5, 1), Some(5));

        // Test byte
        assert_eq!(skip_header_value(&headers, 5, 2), Some(6));

        // Test short
        assert_eq!(skip_header_value(&headers, 5, 3), Some(7));

        // Test int
        assert_eq!(skip_header_value(&headers, 5, 4), Some(9));

        // Test long
        assert_eq!(skip_header_value(&headers, 5, 5), Some(13));

        // Test timestamp
        assert_eq!(skip_header_value(&headers, 5, 8), Some(13));

        // Test uuid
        assert_eq!(skip_header_value(&headers, 5, 9), Some(21));
    }

    #[test]
    fn test_parse_empty_payload() {
        let result = parse_payload(&[]);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Value::Null);
    }

    #[test]
    fn test_parse_json_payload() {
        let json = br#"{"content":"hello","type":"text"}"#;
        let result = parse_payload(json);
        assert!(result.is_ok());
        let value = result.unwrap();
        assert_eq!(value["content"], "hello");
        assert_eq!(value["type"], "text");
    }
}
