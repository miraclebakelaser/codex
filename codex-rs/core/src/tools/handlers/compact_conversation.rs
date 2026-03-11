use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::LazyLock;

use async_trait::async_trait;
use serde::Deserialize;

use crate::client_common::tools::ResponsesApiTool;
use crate::client_common::tools::ToolSpec;
use crate::compact::InitialContextInjection;
use crate::compact::run_auto_compact;
use crate::function_tool::FunctionCallError;
use crate::tools::context::FunctionToolOutput;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;
use crate::tools::spec::JsonSchema;
use codex_protocol::models::ResponseItem;

pub struct CompactConversationHandler;

pub static COMPACT_CONVERSATION_TOOL: LazyLock<ToolSpec> = LazyLock::new(|| {
    ToolSpec::Function(ResponsesApiTool {
        name: "compact_conversation".to_string(),
        description: "Compacts the current conversation history and continues from the shortened history. Use this when the thread is getting long or the user explicitly asks you to compact. Call it only when useful, not routinely.".to_string(),
        strict: false,
        parameters: JsonSchema::Object {
            properties: BTreeMap::new(),
            required: None,
            additional_properties: Some(false.into()),
        },
        output_schema: None,
    })
});

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CompactConversationArgs {}

#[async_trait]
impl ToolHandler for CompactConversationHandler {
    type Output = FunctionToolOutput;

    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<Self::Output, FunctionCallError> {
        let ToolInvocation {
            session,
            turn,
            call_id,
            payload,
            ..
        } = invocation;

        let arguments = match payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "compact_conversation handler received unsupported payload".to_string(),
                ));
            }
        };

        let _args: CompactConversationArgs = parse_arguments(&arguments)?;
        let function_call_item = session
            .clone_history()
            .await
            .raw_items()
            .iter()
            .rev()
            .find_map(|item| match item {
                ResponseItem::FunctionCall {
                    call_id: existing_call_id,
                    ..
                } if existing_call_id == &call_id => Some(item.clone()),
                _ => None,
            });
        run_auto_compact(
            Arc::clone(&session),
            Arc::clone(&turn),
            InitialContextInjection::BeforeLastUserMessage,
        )
        .await
        .map_err(|err| FunctionCallError::RespondToModel(format!("compaction failed: {err}")))?;
        if let Some(function_call_item) = function_call_item {
            let function_call_was_preserved =
                session
                    .clone_history()
                    .await
                    .raw_items()
                    .iter()
                    .any(|item| {
                        matches!(
                            item,
                            ResponseItem::FunctionCall {
                                call_id: existing_call_id,
                                ..
                            } if existing_call_id == &call_id
                        )
                    });
            if !function_call_was_preserved {
                session
                    .record_conversation_items(turn.as_ref(), &[function_call_item])
                    .await;
            }
        }

        Ok(FunctionToolOutput::from_text(
            "Conversation compacted".to_string(),
            Some(true),
        ))
    }
}
