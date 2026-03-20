use crate::config::AppConfig;
use crate::postgres::SymbolRecord;
use anyhow::{Result, anyhow};
use serde_json::{Value, json};
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
    let symbols = collect_symbols(language, root, bytes);
    let diagnostics = json!(collect_diagnostics(root, bytes));
    let metrics = collect_metrics(content, root);
    let chunks = collect_chunks(cfg, language, root, bytes, &symbols);

    Ok(SyntaxAnalysis {
        structure,
        imports,
        exports,
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
                metadata: json!({
                    "language": language,
                    "node_kind": node.kind(),
                    "text": snippet(node, bytes, 240)
                }),
            })
        })
        .collect()
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
