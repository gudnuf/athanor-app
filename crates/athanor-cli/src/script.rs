//! Parse a JSON session script into the engine's `AcpUpdate` stream.
//!
//! `AcpUpdate` is a seam type (no `serde` derive), so the CLI owns this tiny
//! adapter from a human-writable JSON file to the update sequence a
//! `MockEngine` replays. Pure shell wiring — no domain logic.
//!
//! Script shape: a JSON array of update objects, one of:
//! ```json
//! [
//!   { "text": "That thing about entropy you left hanging…" },
//!   { "tool": "fix_salt", "id": "1",
//!     "args": { "realization": "forgetting costs energy", "thread_id": "…" } },
//!   { "complete": true }
//! ]
//! ```

use athanor_core::engine::{AcpToolCall, AcpUpdate};
use serde_json::Value;

type BoxErr = Box<dyn std::error::Error>;

/// Parses a JSON script string into the `AcpUpdate` sequence to replay.
pub fn parse_script(json: &str) -> Result<Vec<AcpUpdate>, BoxErr> {
    let value: Value = serde_json::from_str(json)?;
    let items = value
        .as_array()
        .ok_or("script must be a JSON array of update objects")?;

    let mut updates = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        updates.push(parse_update(item, i)?);
    }
    Ok(updates)
}

fn parse_update(item: &Value, index: usize) -> Result<AcpUpdate, BoxErr> {
    let obj = item
        .as_object()
        .ok_or_else(|| format!("script[{index}] must be an object"))?;

    if let Some(text) = obj.get("text") {
        let text = text
            .as_str()
            .ok_or_else(|| format!("script[{index}].text must be a string"))?;
        return Ok(AcpUpdate::TextDelta(text.to_string()));
    }

    if let Some(tool) = obj.get("tool") {
        let name = tool
            .as_str()
            .ok_or_else(|| format!("script[{index}].tool must be a string"))?;
        let id = obj
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| index.to_string());
        let args = obj.get("args").cloned().unwrap_or(Value::Null);
        return Ok(AcpUpdate::ToolCall(AcpToolCall {
            id,
            name: name.to_string(),
            args,
        }));
    }

    if obj
        .get("complete")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Ok(AcpUpdate::TurnComplete);
    }

    Err(
        format!("script[{index}] must have one of: \"text\", \"tool\", or \"complete\": true")
            .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_tool_and_complete() {
        let json = r#"[
            { "text": "Consider entropy." },
            { "tool": "fix_salt", "id": "7", "args": { "realization": "r", "thread_id": "t" } },
            { "complete": true }
        ]"#;
        let updates = parse_script(json).unwrap();
        assert_eq!(updates.len(), 3);
        assert!(matches!(&updates[0], AcpUpdate::TextDelta(t) if t == "Consider entropy."));
        match &updates[1] {
            AcpUpdate::ToolCall(c) => {
                assert_eq!(c.name, "fix_salt");
                assert_eq!(c.id, "7");
                assert_eq!(c.args["thread_id"], "t");
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
        assert!(matches!(&updates[2], AcpUpdate::TurnComplete));
    }

    #[test]
    fn tool_id_defaults_to_index_when_absent() {
        let json = r#"[ { "tool": "open_thread", "args": { "question": "why?" } } ]"#;
        let updates = parse_script(json).unwrap();
        match &updates[0] {
            AcpUpdate::ToolCall(c) => assert_eq!(c.id, "0"),
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn rejects_non_array_and_unknown_shapes() {
        assert!(parse_script(r#"{ "text": "no" }"#).is_err());
        assert!(parse_script(r#"[ { "mystery": 1 } ]"#).is_err());
    }
}
