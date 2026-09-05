//! Cross-boundary values and the persisted assistant-message contract.
//! Message payloads retain additive frontend fields as JSON, but the stable
//! role/block shape used by Rust readers is checked here once.

use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use thiserror::Error;

use super::ai_tools::Task;

pub const CHAT_MESSAGE_SCHEMA_VERSION: u64 = 1;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ChatMessageContractError {
    #[error("persisted chat message schema version {0} is not supported")]
    UnsupportedVersion(u64),
    #[error("invalid persisted chat message: {0}")]
    Invalid(String),
}

/// Marks a message that stands in for a stored row this build cannot read,
/// and carries that row verbatim so it survives the load → render → save
/// round trip. Reserved: a real frontend message never sets it.
pub const UNREADABLE_SOURCE_FIELD: &str = "unreadableSource";

/// Current persisted envelope. Deserialization also accepts a legacy direct
/// message object; serialization always emits the current version so a normal
/// re-save upgrades old data without a destructive database rewrite.
#[derive(Debug, Clone, PartialEq)]
pub struct PersistedChatMessage {
    /// Always contract-valid, and always what the frontend receives — a
    /// quarantined row is represented here by its readable placeholder.
    message: Value,
    /// The untouched stored row, when `message` only stands in for it.
    /// [`Self::storage_json`] writes this back instead of the placeholder,
    /// so an unreadable message is never *overwritten* by the ordinary save
    /// that follows opening its chat.
    source: Option<Value>,
}

impl PersistedChatMessage {
    pub fn new(message: Value) -> Result<Self, ChatMessageContractError> {
        // The placeholder is what a *previous* load produced for a row that
        // failed this same validation. It is contract-valid itself, so it
        // still normalizes — the field only says which row to write back.
        let source = message
            .as_object()
            .and_then(|object| object.get(UNREADABLE_SOURCE_FIELD))
            .cloned();
        Ok(Self {
            message: normalize_message(message)?,
            source,
        })
    }

    /// A readable stand-in for one stored row that does not satisfy the
    /// contract — a future `schemaVersion`, a block shape this build does
    /// not know, or JSON that no longer parses.
    ///
    /// A chat is a user's own record: one bad row must not make the rest of
    /// it unreachable (which a hard failure on load would do), and it must
    /// not be silently discarded either. So the transcript shows why this
    /// entry is missing, and `source` rides along untouched.
    pub fn quarantined(source: &Value, detail: &str) -> Self {
        let id = format!("unreadable:{}", short_digest(source));
        Self {
            message: serde_json::json!({
                "id": id,
                "role": "assistant",
                "blocks": [{
                    "type": "text",
                    "id": format!("{id}:text"),
                    "content": format!(
                        "⚠️ Это сообщение не удалось прочитать, оно сохранено без изменений: {detail}"
                    ),
                }],
                "failed": true,
                "errorMessage": detail,
                UNREADABLE_SOURCE_FIELD: source.clone(),
            }),
            source: Some(source.clone()),
        }
    }

    pub fn message(&self) -> &Value {
        &self.message
    }

    /// True for a stand-in produced by [`Self::quarantined`] — the memory
    /// pipeline skips these rather than extracting facts from the warning
    /// text they render.
    pub fn is_unreadable(&self) -> bool {
        self.source.is_some()
    }

    /// What belongs in the database for this message — the current envelope
    /// normally, and for a quarantined row the original bytes it stands in
    /// for. Distinct from `Serialize`, which is the frontend's view: there,
    /// a quarantined row must be the readable placeholder instead.
    pub fn storage_json(&self) -> Result<String, serde_json::Error> {
        match &self.source {
            Some(source) => serde_json::to_string(source),
            None => serde_json::to_string(self),
        }
    }
}

/// Stable short id for a quarantined row, so reopening the same chat does
/// not reshuffle React keys. Not a security digest — just FNV-1a over the
/// row's own bytes.
fn short_digest(source: &Value) -> String {
    let rendered = source.to_string();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in rendered.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

impl Serialize for PersistedChatMessage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Envelope<'a> {
            schema_version: u64,
            message: &'a Value,
        }

        Envelope {
            schema_version: CHAT_MESSAGE_SCHEMA_VERSION,
            message: &self.message,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PersistedChatMessage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = Value::deserialize(deserializer)?;
        let message = match wire.as_object() {
            Some(object) if object.contains_key("schemaVersion") => {
                let version = object
                    .get("schemaVersion")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| D::Error::custom("schemaVersion must be an unsigned integer"))?;
                if version != CHAT_MESSAGE_SCHEMA_VERSION {
                    return Err(D::Error::custom(
                        ChatMessageContractError::UnsupportedVersion(version),
                    ));
                }
                object.get("message").cloned().ok_or_else(|| {
                    D::Error::custom("versioned chat message has no message field")
                })?
            }
            _ => wire,
        };
        Self::new(message).map_err(D::Error::custom)
    }
}

fn required_string<'a>(
    object: &'a serde_json::Map<String, Value>,
    field: &str,
    path: &str,
) -> Result<&'a str, ChatMessageContractError> {
    object.get(field).and_then(Value::as_str).ok_or_else(|| {
        ChatMessageContractError::Invalid(format!("{path}.{field} must be a string"))
    })
}

fn validate_block(block: &Value, index: usize) -> Result<(), ChatMessageContractError> {
    let path = format!("assistant.blocks[{index}]");
    let object = block
        .as_object()
        .ok_or_else(|| ChatMessageContractError::Invalid(format!("{path} must be an object")))?;
    required_string(object, "id", &path)?;
    match required_string(object, "type", &path)? {
        "text" | "reasoning" => {
            required_string(object, "content", &path)?;
        }
        "steer" => {
            required_string(object, "text", &path)?;
        }
        "toolCall" => {
            required_string(object, "name", &path)?;
            required_string(object, "argumentsJson", &path)?;
            match required_string(object, "status", &path)? {
                "pendingApproval" | "running" | "done" | "error" => {}
                _ => {
                    return Err(ChatMessageContractError::Invalid(format!(
                        "{path}.status is not supported"
                    )));
                }
            }
        }
        _ => {
            return Err(ChatMessageContractError::Invalid(format!(
                "{path}.type is not supported"
            )));
        }
    }
    Ok(())
}

fn normalize_message(mut message: Value) -> Result<Value, ChatMessageContractError> {
    let object = message
        .as_object_mut()
        .ok_or_else(|| ChatMessageContractError::Invalid("message must be an object".into()))?;
    let id = required_string(object, "id", "message")?.to_string();
    match required_string(object, "role", "message")? {
        "user" => {
            required_string(object, "content", "user")?;
        }
        "assistant" => {
            if let Some(blocks) = object.get("blocks").and_then(Value::as_array) {
                for (index, block) in blocks.iter().enumerate() {
                    validate_block(block, index)?;
                }
            } else if let Some(content) = object
                .remove("content")
                .and_then(|value| value.as_str().map(str::to_owned))
            {
                object.insert(
                    "blocks".into(),
                    serde_json::json!([{
                        "type": "text",
                        "id": format!("{id}:legacy-text"),
                        "content": content
                    }]),
                );
            } else {
                return Err(ChatMessageContractError::Invalid(
                    "assistant.blocks must be an array".into(),
                ));
            }
        }
        _ => {
            return Err(ChatMessageContractError::Invalid(
                "message.role must be user or assistant".into(),
            ));
        }
    }
    Ok(message)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSummary {
    pub id: String,
    pub repo_root: String,
    pub title: String,
    pub archived: bool,
    /// Unix milliseconds.
    pub created_at: i64,
    /// Unix milliseconds.
    pub updated_at: i64,
}

/// One chat's full persisted state plus its typed todo checklist.
/// Combined into one round-trip type rather than a separate "load todos"
/// call: every caller (`useChatHistory`'s mount effect, `switchChat`)
/// always needs both together.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedChat {
    pub messages: Vec<PersistedChatMessage>,
    pub todos: Vec<Task>,
    /// Active work-plan id for this chat (from Plan-mode createPlan /
    /// Start), if any.
    pub active_plan_id: Option<String>,
    /// Opaque `PendingApproval` blob (same trust boundary as `messages` —
    /// never parsed here) when this chat was last saved mid-turn, paused
    /// awaiting a tool-approval/`askUser` decision that was never resolved
    /// before the app closed. `None` for a chat with no unresolved pause.
    /// Lets the frontend resume via `llm_chat_stream_resume` after a full
    /// app restart, not just a same-session panel close.
    pub pending_resume: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_flat_assistant_is_normalized_and_serialized_as_version_one() {
        let persisted: PersistedChatMessage = serde_json::from_value(serde_json::json!({
            "id": "assistant-1",
            "role": "assistant",
            "content": "legacy answer"
        }))
        .unwrap();

        assert_eq!(
            persisted.message()["blocks"][0],
            serde_json::json!({
                "type": "text",
                "id": "assistant-1:legacy-text",
                "content": "legacy answer"
            })
        );
        assert_eq!(
            serde_json::to_value(persisted).unwrap()["schemaVersion"],
            CHAT_MESSAGE_SCHEMA_VERSION
        );
    }

    #[test]
    fn unknown_persisted_message_version_is_rejected() {
        let error = serde_json::from_value::<PersistedChatMessage>(serde_json::json!({
            "schemaVersion": 2,
            "message": {"id": "u1", "role": "user", "content": "future"}
        }))
        .unwrap_err();

        assert!(error.to_string().contains("version 2 is not supported"));
    }

    #[test]
    fn a_quarantined_row_renders_as_a_readable_assistant_message() {
        let source = serde_json::json!({"schemaVersion": 2, "message": {"role": "oracle"}});
        let placeholder = PersistedChatMessage::quarantined(&source, "version 2 is not supported");

        assert!(placeholder.is_unreadable());
        assert_eq!(placeholder.message()["role"], "assistant");
        assert_eq!(placeholder.message()["blocks"][0]["type"], "text");
        assert!(placeholder.message()["blocks"][0]["content"]
            .as_str()
            .unwrap()
            .contains("version 2 is not supported"));
        // What the frontend receives is an ordinary current-version message,
        // so its own decoder does not have to know about quarantine at all.
        assert_eq!(
            serde_json::to_value(&placeholder).unwrap()["schemaVersion"],
            CHAT_MESSAGE_SCHEMA_VERSION
        );
    }

    #[test]
    fn saving_a_quarantined_row_back_restores_it_byte_for_byte() {
        let source = serde_json::json!({"schemaVersion": 2, "message": {"role": "oracle"}});
        let placeholder = PersistedChatMessage::quarantined(&source, "unsupported");

        // Exactly the load → frontend → save round trip: the placeholder is
        // serialized out, comes back in, and must write the original row.
        let to_frontend = serde_json::to_value(&placeholder).unwrap();
        let from_frontend: PersistedChatMessage = serde_json::from_value(to_frontend).unwrap();

        assert!(from_frontend.is_unreadable());
        assert_eq!(
            serde_json::from_str::<Value>(&from_frontend.storage_json().unwrap()).unwrap(),
            source
        );
        // And a load that never reached the frontend writes it back too.
        assert_eq!(
            serde_json::from_str::<Value>(&placeholder.storage_json().unwrap()).unwrap(),
            source
        );
    }

    #[test]
    fn a_quarantined_id_is_stable_across_loads() {
        let source = serde_json::json!({"schemaVersion": 2, "message": {"role": "oracle"}});
        assert_eq!(
            PersistedChatMessage::quarantined(&source, "unsupported").message()["id"],
            PersistedChatMessage::quarantined(&source, "unsupported").message()["id"]
        );
    }
}
