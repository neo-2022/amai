use crate::config::AppConfig;
use crate::postgres::SymbolRecord;
use anyhow::{Result, anyhow};
use serde_json::{Map, Value, json};
use tree_sitter::{Language, Node, Parser};

#[derive(Debug, Clone)]
pub struct SyntaxChunk {
    pub chunk_index: i32,
    pub total_chunks: i32,
    pub start_line: i32,
    pub end_line: i32,
    pub start_byte: i32,
    pub end_byte: i32,
    pub content: String,
    pub metadata: Value,
}

#[derive(Debug, Clone)]
pub struct SyntaxAnalysis {
    pub structure: Value,
    pub imports: Value,
    pub exports: Value,
    pub call_references: Value,
    pub diagnostics: Value,
    pub symbols: Vec<SymbolRecord>,
    pub chunks: Vec<SyntaxChunk>,
    pub metrics: Value,
}

pub fn supports(language: &str) -> bool {
    resolve_language(language).is_some()
}

pub fn analyze(cfg: &AppConfig, language: &str, content: &str) -> Result<SyntaxAnalysis> {
    let parser_language = resolve_language(language)
        .ok_or_else(|| anyhow!("unsupported tree-sitter language: {language}"))?;
    let mut parser = Parser::new();
    parser
        .set_language(&parser_language)
        .map_err(|error| anyhow!("failed to load parser for {language}: {error}"))?;
    let tree = parser
        .parse(content, None)
        .ok_or_else(|| anyhow!("tree-sitter returned no parse tree for {language}"))?;
    let root = tree.root_node();
    let bytes = content.as_bytes();

    let structure = json!(collect_structure(language, root, bytes));
    let imports = json!(collect_nodes(language, root, bytes, import_kinds(language)));
    let exports = json!(collect_nodes(language, root, bytes, export_kinds(language)));
    let call_references = json!(collect_call_references(language, root, bytes));
    let symbols = collect_symbols(language, root, bytes);
    let diagnostics = json!(collect_diagnostics(root, bytes));
    let metrics = collect_metrics(content, root);
    let chunks = collect_chunks(cfg, language, root, bytes, &symbols);

    Ok(SyntaxAnalysis {
        structure,
        imports,
        exports,
        call_references,
        diagnostics,
        symbols,
        chunks,
        metrics,
    })
}

fn resolve_language(language: &str) -> Option<Language> {
    match language {
        "rust" => Some(tree_sitter_rust::LANGUAGE.into()),
        "toml" => Some(tree_sitter_toml_ng::LANGUAGE.into()),
        "javascript" => Some(tree_sitter_javascript::LANGUAGE.into()),
        "typescript" => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
        "tsx" => Some(tree_sitter_typescript::LANGUAGE_TSX.into()),
        "json" => Some(tree_sitter_json::LANGUAGE.into()),
        _ => None,
    }
}

fn collect_structure(language: &str, root: Node<'_>, bytes: &[u8]) -> Vec<Value> {
    let kinds = structure_kinds(language);
    traverse(root)
        .into_iter()
        .filter(|node| kinds.contains(&node.kind()))
        .map(|node| {
            json!({
                "kind": node.kind(),
                "name": node_name(language, node, bytes),
                "start_line": (node.start_position().row + 1) as i32,
                "end_line": (node.end_position().row + 1) as i32,
                "start_byte": node.start_byte() as i32,
                "end_byte": node.end_byte() as i32
            })
        })
        .collect()
}

fn collect_nodes(language: &str, root: Node<'_>, bytes: &[u8], kinds: &[&str]) -> Vec<Value> {
    traverse(root)
        .into_iter()
        .filter(|node| kinds.contains(&node.kind()))
        .map(|node| {
            json!({
                "kind": node.kind(),
                "text": snippet(node, bytes, 240),
                "start_line": (node.start_position().row + 1) as i32,
                "end_line": (node.end_position().row + 1) as i32,
                "name": node_name(language, node, bytes)
            })
        })
        .collect()
}

fn collect_symbols(language: &str, root: Node<'_>, bytes: &[u8]) -> Vec<SymbolRecord> {
    let kinds = symbol_kinds(language);
    traverse(root)
        .into_iter()
        .filter(|node| kinds.contains(&node.kind()))
        .filter_map(|node| {
            let name = node_name(language, node, bytes)?;
            Some(SymbolRecord {
                name,
                kind: node.kind().to_string(),
                start_line: (node.start_position().row + 1) as i32,
                end_line: (node.end_position().row + 1) as i32,
                start_byte: node.start_byte() as i32,
                end_byte: node.end_byte() as i32,
                metadata: symbol_metadata(language, node, bytes),
            })
        })
        .collect()
}

fn symbol_metadata(language: &str, node: Node<'_>, bytes: &[u8]) -> Value {
    let mut metadata = Map::new();
    metadata.insert("language".to_string(), json!(language));
    metadata.insert("node_kind".to_string(), json!(node.kind()));
    metadata.insert("text".to_string(), json!(snippet(node, bytes, 240)));
    if language == "rust" {
        if let Some(owner) = rust_symbol_owner_context(node, bytes) {
            metadata.insert("owner_kind".to_string(), json!(owner.owner_kind));
            metadata.insert("owner_name".to_string(), json!(owner.owner_name));
            metadata.insert("owner_path".to_string(), json!(owner.owner_path));
            if let Some(trait_name) = owner.trait_name {
                metadata.insert("trait_name".to_string(), json!(trait_name));
            }
        }
    }
    Value::Object(metadata)
}

#[derive(Debug, Clone)]
struct RustSymbolOwnerContext {
    owner_kind: String,
    owner_name: String,
    owner_path: String,
    trait_name: Option<String>,
}

fn rust_symbol_owner_context(node: Node<'_>, bytes: &[u8]) -> Option<RustSymbolOwnerContext> {
    let mut cursor = node.parent();
    while let Some(parent) = cursor {
        match parent.kind() {
            "impl_item" => {
                let owner_type = parent.child_by_field_name("type")?;
                let owner_name = rust_type_terminal_name(owner_type, bytes)?;
                let owner_path = rust_type_path(owner_type, bytes)?;
                let trait_name = parent
                    .child_by_field_name("trait")
                    .and_then(|trait_node| rust_type_path(trait_node, bytes));
                return Some(RustSymbolOwnerContext {
                    owner_kind: "impl_item".to_string(),
                    owner_name,
                    owner_path,
                    trait_name,
                });
            }
            "trait_item" => {
                let owner_name = node_name("rust", parent, bytes)?;
                return Some(RustSymbolOwnerContext {
                    owner_kind: "trait_item".to_string(),
                    owner_path: owner_name.clone(),
                    owner_name,
                    trait_name: None,
                });
            }
            _ => {
                cursor = parent.parent();
            }
        }
    }
    None
}

fn rust_type_path(node: Node<'_>, bytes: &[u8]) -> Option<String> {
    match node.kind() {
        "generic_type" | "generic_type_with_turbofish" => node
            .child_by_field_name("type")
            .and_then(|inner| rust_type_path(inner, bytes)),
        "identifier" | "type_identifier" | "scoped_identifier" | "scoped_type_identifier" => {
            Some(trimmed_text(node, bytes))
        }
        _ => {
            let value = trimmed_text(node, bytes);
            (!value.is_empty()).then_some(value)
        }
    }
}

fn rust_type_terminal_name(node: Node<'_>, bytes: &[u8]) -> Option<String> {
    match node.kind() {
        "generic_type" | "generic_type_with_turbofish" => node
            .child_by_field_name("type")
            .and_then(|inner| rust_type_terminal_name(inner, bytes)),
        "scoped_type_identifier" | "scoped_identifier" => node
            .child_by_field_name("name")
            .map(|name| trimmed_text(name, bytes)),
        "identifier" | "type_identifier" => Some(trimmed_text(node, bytes)),
        _ => rust_type_path(node, bytes).map(|path| {
            path.split("::")
                .filter(|segment| !segment.is_empty())
                .last()
                .unwrap_or(path.as_str())
                .to_string()
        }),
    }
}

fn collect_call_references(language: &str, root: Node<'_>, bytes: &[u8]) -> Vec<Value> {
    match language {
        "rust" => collect_rust_call_references(root, bytes),
        _ => Vec::new(),
    }
}

fn collect_rust_call_references(root: Node<'_>, bytes: &[u8]) -> Vec<Value> {
    traverse(root)
        .into_iter()
        .filter_map(|node| match node.kind() {
            "call_expression" => rust_call_reference(node, bytes),
            "macro_invocation" => rust_macro_reference(node, bytes),
            _ => None,
        })
        .collect()
}

fn rust_call_reference(node: Node<'_>, bytes: &[u8]) -> Option<Value> {
    let function = node.child_by_field_name("function")?;
    let is_generic = function.kind() == "generic_function";
    let callee = if is_generic {
        function.child_by_field_name("function").unwrap_or(function)
    } else {
        function
    };
    let call_style = match callee.kind() {
        "identifier" => "identifier",
        "scoped_identifier" => "scoped_identifier",
        "field_expression" => "field_expression",
        _ => return None,
    };
    let callee_name = rust_callee_name(callee, bytes);
    let callee_path = Some(trimmed_text(callee, bytes));
    let receiver_text = if callee.kind() == "field_expression" {
        callee
            .child_by_field_name("value")
            .map(|value| snippet(value, bytes, 160))
    } else {
        None
    };
    let owner_context = rust_symbol_owner_context(node, bytes);
    Some(json!({
        "kind": node.kind(),
        "call_style": call_style,
        "callee_name": callee_name,
        "callee_path": callee_path,
        "receiver_text": receiver_text,
        "enclosing_owner_kind": owner_context.as_ref().map(|owner| owner.owner_kind.clone()),
        "enclosing_owner_name": owner_context.as_ref().map(|owner| owner.owner_name.clone()),
        "enclosing_owner_path": owner_context.as_ref().map(|owner| owner.owner_path.clone()),
        "enclosing_trait_name": owner_context.as_ref().and_then(|owner| owner.trait_name.clone()),
        "generic": is_generic,
        "start_line": (node.start_position().row + 1) as i32,
        "end_line": (node.end_position().row + 1) as i32,
        "text": snippet(node, bytes, 240),
    }))
}

fn rust_macro_reference(node: Node<'_>, bytes: &[u8]) -> Option<Value> {
    let macro_node = node.child_by_field_name("macro")?;
    let call_style = match macro_node.kind() {
        "identifier" => "macro_identifier",
        "scoped_identifier" => "macro_scoped_identifier",
        _ => return None,
    };
    let owner_context = rust_symbol_owner_context(node, bytes);
    Some(json!({
        "kind": node.kind(),
        "call_style": call_style,
        "callee_name": rust_callee_name(macro_node, bytes),
        "callee_path": trimmed_text(macro_node, bytes),
        "receiver_text": Value::Null,
        "enclosing_owner_kind": owner_context.as_ref().map(|owner| owner.owner_kind.clone()),
        "enclosing_owner_name": owner_context.as_ref().map(|owner| owner.owner_name.clone()),
        "enclosing_owner_path": owner_context.as_ref().map(|owner| owner.owner_path.clone()),
        "enclosing_trait_name": owner_context.as_ref().and_then(|owner| owner.trait_name.clone()),
        "generic": false,
        "start_line": (node.start_position().row + 1) as i32,
        "end_line": (node.end_position().row + 1) as i32,
        "text": snippet(node, bytes, 240),
    }))
}

fn rust_callee_name(node: Node<'_>, bytes: &[u8]) -> Option<String> {
    match node.kind() {
        "identifier" => Some(trimmed_text(node, bytes)),
        "scoped_identifier" => node
            .child_by_field_name("name")
            .map(|name| trimmed_text(name, bytes)),
        "field_expression" => node
            .child_by_field_name("field")
            .map(|field| trimmed_text(field, bytes)),
        _ => None,
    }
}

fn collect_diagnostics(root: Node<'_>, bytes: &[u8]) -> Vec<Value> {
    traverse(root)
        .into_iter()
        .filter(|node| node.is_error() || node.is_missing())
        .map(|node| {
            json!({
                "severity": "error",
                "kind": node.kind(),
                "start_line": (node.start_position().row + 1) as i32,
                "end_line": (node.end_position().row + 1) as i32,
                "start_byte": node.start_byte() as i32,
                "end_byte": node.end_byte() as i32,
                "text": snippet(node, bytes, 240)
            })
        })
        .collect()
}

fn collect_metrics(content: &str, root: Node<'_>) -> Value {
    let total_lines = content.lines().count();
    let blank_lines = content
        .lines()
        .filter(|line| line.trim().is_empty())
        .count();
    let mut node_count = 0usize;
    let mut error_count = 0usize;
    let mut max_depth = 0usize;
    for (node, depth) in traverse_with_depth(root) {
        node_count += 1;
        if node.is_error() || node.is_missing() {
            error_count += 1;
        }
        max_depth = max_depth.max(depth);
    }

    json!({
        "total_lines": total_lines,
        "code_lines": total_lines.saturating_sub(blank_lines),
        "comment_lines": 0,
        "blank_lines": blank_lines,
        "total_bytes": content.len(),
        "node_count": node_count,
        "error_count": error_count,
        "max_depth": max_depth
    })
}

fn collect_chunks(
    cfg: &AppConfig,
    language: &str,
    root: Node<'_>,
    bytes: &[u8],
    symbols: &[SymbolRecord],
) -> Vec<SyntaxChunk> {
    let mut nodes = Vec::new();
    let mut cursor = root.walk();
    let children = root.named_children(&mut cursor).collect::<Vec<_>>();
    if children.is_empty() {
        nodes.push(root);
    } else {
        for child in children {
            append_chunk_nodes(child, cfg.chunk_max_bytes, &mut nodes);
        }
    }

    let mut chunks = nodes
        .into_iter()
        .filter(|node| node.end_byte() > node.start_byte())
        .filter_map(|node| {
            let content = node.utf8_text(bytes).ok()?.trim().to_string();
            if content.is_empty() {
                return None;
            }
            let start_byte = node.start_byte() as i32;
            let end_byte = node.end_byte() as i32;
            let symbols_defined = symbols
                .iter()
                .filter(|symbol| symbol.start_byte >= start_byte && symbol.end_byte <= end_byte)
                .map(|symbol| symbol.name.clone())
                .collect::<Vec<_>>();
            Some(SyntaxChunk {
                chunk_index: 0,
                total_chunks: 0,
                start_line: (node.start_position().row + 1) as i32,
                end_line: (node.end_position().row + 1) as i32,
                start_byte,
                end_byte,
                content,
                metadata: json!({
                    "language": language,
                    "node_types": [node.kind()],
                    "context_path": [],
                    "symbols_defined": symbols_defined,
                    "has_error_nodes": node.has_error()
                }),
            })
        })
        .collect::<Vec<_>>();

    let total_chunks = chunks.len() as i32;
    for (index, chunk) in chunks.iter_mut().enumerate() {
        chunk.chunk_index = index as i32;
        chunk.total_chunks = total_chunks;
    }
    chunks
}

fn append_chunk_nodes<'a>(node: Node<'a>, max_bytes: usize, out: &mut Vec<Node<'a>>) {
    let node_size = node.end_byte().saturating_sub(node.start_byte());
    if node_size <= max_bytes || node.named_child_count() == 0 {
        out.push(node);
        return;
    }

    let mut cursor = node.walk();
    let children = node.named_children(&mut cursor).collect::<Vec<_>>();
    if children.is_empty() {
        out.push(node);
        return;
    }

    for child in children {
        append_chunk_nodes(child, max_bytes, out);
    }
}

fn traverse(root: Node<'_>) -> Vec<Node<'_>> {
    traverse_with_depth(root)
        .into_iter()
        .map(|(node, _)| node)
        .collect()
}

fn traverse_with_depth(root: Node<'_>) -> Vec<(Node<'_>, usize)> {
    let mut stack = vec![(root, 0usize)];
    let mut nodes = Vec::new();
    while let Some((node, depth)) = stack.pop() {
        nodes.push((node, depth));
        let mut cursor = node.walk();
        let children = node.named_children(&mut cursor).collect::<Vec<_>>();
        for child in children.into_iter().rev() {
            stack.push((child, depth + 1));
        }
    }
    nodes
}

fn node_name(language: &str, node: Node<'_>, bytes: &[u8]) -> Option<String> {
    if let Some(name_node) = node.child_by_field_name("name") {
        return Some(trimmed_text(name_node, bytes));
    }

    match (language, node.kind()) {
        ("toml", "pair") | ("toml", "table") | ("toml", "array_table") => {
            let mut cursor = node.walk();
            node.named_children(&mut cursor)
                .next()
                .map(|child| trimmed_text(child, bytes))
        }
        _ => None,
    }
}

fn snippet(node: Node<'_>, bytes: &[u8], max_chars: usize) -> String {
    let text = node.utf8_text(bytes).unwrap_or("").trim();
    if text.chars().count() <= max_chars {
        text.to_string()
    } else {
        text.chars().take(max_chars).collect::<String>()
    }
}

fn trimmed_text(node: Node<'_>, bytes: &[u8]) -> String {
    node.utf8_text(bytes).unwrap_or("").trim().to_string()
}

fn structure_kinds(language: &str) -> &'static [&'static str] {
    match language {
        "rust" => &[
            "function_item",
            "struct_item",
            "enum_item",
            "trait_item",
            "impl_item",
            "mod_item",
            "const_item",
            "static_item",
            "type_item",
            "macro_definition",
        ],
        "toml" => &["table", "array_table", "pair"],
        "javascript" | "typescript" | "tsx" => &[
            "function_declaration",
            "class_declaration",
            "method_definition",
            "interface_declaration",
            "type_alias_declaration",
            "enum_declaration",
        ],
        "json" => &["pair", "object", "array"],
        _ => &[],
    }
}

fn symbol_kinds(language: &str) -> &'static [&'static str] {
    match language {
        "rust" => &[
            "function_item",
            "struct_item",
            "enum_item",
            "trait_item",
            "impl_item",
            "mod_item",
            "const_item",
            "static_item",
            "type_item",
        ],
        "toml" => &["pair", "table", "array_table"],
        "javascript" | "typescript" | "tsx" => &[
            "function_declaration",
            "class_declaration",
            "method_definition",
            "interface_declaration",
            "type_alias_declaration",
            "enum_declaration",
            "lexical_declaration",
            "variable_declarator",
        ],
        "json" => &["pair"],
        _ => &[],
    }
}

fn import_kinds(language: &str) -> &'static [&'static str] {
    match language {
        "rust" => &["use_declaration"],
        "javascript" | "typescript" | "tsx" => &["import_statement"],
        _ => &[],
    }
}

fn export_kinds(language: &str) -> &'static [&'static str] {
    match language {
        "javascript" | "typescript" | "tsx" => &[
            "export_statement",
            "export_clause",
            "export_default_declaration",
        ],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::analyze;
    use crate::config::AppConfig;
    use serde_json::json;

    fn test_config() -> AppConfig {
        AppConfig {
            stack_name: "amai".to_string(),
            pg_db: "amai".to_string(),
            app_db_user: "amai".to_string(),
            app_db_password: "amai".to_string(),
            postgres_dsn: "postgres://localhost/unused".to_string(),
            app_postgres_dsn: "postgres://localhost/unused".to_string(),
            qdrant_url: "http://127.0.0.1:6334".to_string(),
            qdrant_http_url: "http://127.0.0.1:6334".to_string(),
            qdrant_collection_code: "test".to_string(),
            benchmark_qdrant_http_url: None,
            benchmark_qdrant_collection_code: None,
            qdrant_alias_code: "test".to_string(),
            qdrant_collection_memory: "memory".to_string(),
            qdrant_alias_memory: "memory".to_string(),
            qdrant_code_dim: 384,
            qdrant_memory_dim: 384,
            qdrant_distance: "Cosine".to_string(),
            s3_endpoint: "http://127.0.0.1:9000".to_string(),
            s3_region: "us-east-1".to_string(),
            s3_access_key: "test".to_string(),
            s3_secret_key: "test".to_string(),
            s3_bucket_artifacts: "artifacts".to_string(),
            s3_bucket_transcripts: "transcripts".to_string(),
            s3_bucket_context: "context".to_string(),
            nats_url: "nats://127.0.0.1:4222".to_string(),
            nats_http_url: "http://127.0.0.1:8222".to_string(),
            edge_cache_path: "/tmp/edge-cache-test.db".into(),
            default_retrieval_mode: "local_strict".to_string(),
            code_embed_model: "multilingual_e5_small".to_string(),
            memory_embed_model: "multilingual_e5_small".to_string(),
            chunk_max_bytes: 512,
            fallback_chunk_lines: 40,
            fallback_chunk_overlap_lines: 5,
            local_fast_cache_ttl_ms: 5_000,
        }
    }

    #[test]
    fn rust_analysis_collects_call_references() {
        let cfg = test_config();
        let analysis = analyze(
            &cfg,
            "rust",
            r#"
mod alpha;
use crate::alpha::beta_name;

pub fn runtime_summary() -> &'static str {
    println!("{}", beta_name());
    beta_name()
}
"#,
        )
        .expect("syntax analysis");
        let calls = analysis
            .call_references
            .as_array()
            .expect("call references");
        assert!(calls.iter().any(|call| {
            call["call_style"] == json!("identifier") && call["callee_name"] == json!("beta_name")
        }));
        assert!(calls.iter().any(|call| {
            call["call_style"] == json!("macro_identifier")
                && call["callee_name"] == json!("println")
        }));
    }

    #[test]
    fn rust_analysis_collects_impl_owner_metadata_for_methods() {
        let cfg = test_config();
        let analysis = analyze(
            &cfg,
            "rust",
            r#"
pub struct Beta;

impl Beta {
    pub fn new() -> Self {
        Self
    }
}
"#,
        )
        .expect("syntax analysis");
        let method = analysis
            .symbols
            .iter()
            .find(|symbol| symbol.name == "new")
            .expect("method symbol");
        assert_eq!(method.metadata["owner_kind"], json!("impl_item"));
        assert_eq!(method.metadata["owner_name"], json!("Beta"));
        assert_eq!(method.metadata["owner_path"], json!("Beta"));
    }

    #[test]
    fn rust_analysis_attaches_enclosing_owner_to_self_calls() {
        let cfg = test_config();
        let analysis = analyze(
            &cfg,
            "rust",
            r#"
pub struct Beta;

impl Beta {
    fn helper(&self) -> Self {
        Self
    }

    pub fn make(&self) -> Self {
        self.helper()
    }
}
"#,
        )
        .expect("syntax analysis");
        let call = analysis
            .call_references
            .as_array()
            .and_then(|calls| {
                calls
                    .iter()
                    .find(|call| call["call_style"] == json!("field_expression"))
            })
            .expect("self call");
        assert_eq!(call["receiver_text"], json!("self"));
        assert_eq!(call["enclosing_owner_kind"], json!("impl_item"));
        assert_eq!(call["enclosing_owner_name"], json!("Beta"));
        assert_eq!(call["enclosing_owner_path"], json!("Beta"));
    }
}
