use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct McpErrorSpec {
    pub(crate) jsonrpc_code: i64,
    pub(crate) message: &'static str,
    pub(crate) amai_error_code: &'static str,
    pub(crate) amai_error_class: &'static str,
    pub(crate) retryable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct McpError {
    pub(crate) spec: McpErrorSpec,
    pub(crate) detail: String,
}

impl McpError {
    pub(crate) fn parse(detail: impl Into<String>) -> Self {
        Self {
            spec: McpErrorSpec {
                jsonrpc_code: -32700,
                message: "invalid JSON-RPC payload",
                amai_error_code: "invalid_json_rpc_payload",
                amai_error_class: "protocol_parse",
                retryable: false,
            },
            detail: detail.into(),
        }
    }

    pub(crate) fn invalid_request(detail: impl Into<String>) -> Self {
        Self {
            spec: McpErrorSpec {
                jsonrpc_code: -32600,
                message: "invalid request",
                amai_error_code: "invalid_request",
                amai_error_class: "protocol_request",
                retryable: false,
            },
            detail: detail.into(),
        }
    }

    pub(crate) fn invalid_params(detail: impl Into<String>) -> Self {
        Self {
            spec: McpErrorSpec {
                jsonrpc_code: -32602,
                message: "invalid params",
                amai_error_code: "invalid_params",
                amai_error_class: "client_input",
                retryable: false,
            },
            detail: detail.into(),
        }
    }

    pub(crate) fn method_not_found(detail: impl Into<String>) -> Self {
        Self {
            spec: McpErrorSpec {
                jsonrpc_code: -32601,
                message: "method not found",
                amai_error_code: "method_not_found",
                amai_error_class: "protocol_dispatch",
                retryable: false,
            },
            detail: detail.into(),
        }
    }

    pub(crate) fn prompt_not_found(detail: impl Into<String>) -> Self {
        Self {
            spec: McpErrorSpec {
                jsonrpc_code: -32601,
                message: "prompt not found",
                amai_error_code: "prompt_not_found",
                amai_error_class: "prompt_dispatch",
                retryable: false,
            },
            detail: detail.into(),
        }
    }

    pub(crate) fn tool_not_found(detail: impl Into<String>) -> Self {
        Self {
            spec: McpErrorSpec {
                jsonrpc_code: -32601,
                message: "tool not found",
                amai_error_code: "tool_not_found",
                amai_error_class: "tool_dispatch",
                retryable: false,
            },
            detail: detail.into(),
        }
    }

    pub(crate) fn tool_runtime(error: anyhow::Error) -> Self {
        Self {
            spec: McpErrorSpec {
                jsonrpc_code: -32000,
                message: "tool execution failed",
                amai_error_code: "tool_execution_failed",
                amai_error_class: "tool_runtime",
                retryable: false,
            },
            detail: format!("{error:#}"),
        }
    }
}

pub(crate) fn mcp_error_taxonomy_payload(error: &McpError) -> Value {
    json!({
        "amai_error_code": error.spec.amai_error_code,
        "amai_error_class": error.spec.amai_error_class,
        "retryable": error.spec.retryable,
        "detail": error.detail,
    })
}

pub(crate) fn mcp_jsonrpc_error_response(id: Value, error: &McpError) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": error.spec.jsonrpc_code,
            "message": error.spec.message,
            "data": mcp_error_taxonomy_payload(error),
        }
    })
}

pub(crate) fn mcp_tool_error_result(error: &McpError) -> Value {
    json!({
        "content": [{
            "type": "text",
            "text": format!("tool failed: {}", error.detail)
        }],
        "isError": true,
        "structuredContent": {
            "error_taxonomy": mcp_error_taxonomy_payload(error),
        }
    })
}
