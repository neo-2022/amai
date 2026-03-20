use std::path::Path;

#[derive(Debug, Clone)]
pub struct LanguageDescriptor {
    pub parser_language: Option<&'static str>,
    pub source_kind: &'static str,
}

pub fn detect(path: &Path) -> Option<LanguageDescriptor> {
    let file_name = path.file_name()?.to_string_lossy().to_lowercase();
    let ext = path
        .extension()
        .map(|ext| ext.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    let desc = match ext.as_str() {
        "rs" => LanguageDescriptor {
            parser_language: Some("rust"),
            source_kind: "code",
        },
        "ts" => LanguageDescriptor {
            parser_language: Some("typescript"),
            source_kind: "code",
        },
        "tsx" => LanguageDescriptor {
            parser_language: Some("tsx"),
            source_kind: "code",
        },
        "js" | "mjs" | "cjs" => LanguageDescriptor {
            parser_language: Some("javascript"),
            source_kind: "code",
        },
        "py" => LanguageDescriptor {
            parser_language: Some("python"),
            source_kind: "code",
        },
        "go" => LanguageDescriptor {
            parser_language: Some("go"),
            source_kind: "code",
        },
        "java" => LanguageDescriptor {
            parser_language: Some("java"),
            source_kind: "code",
        },
        "kt" => LanguageDescriptor {
            parser_language: Some("kotlin"),
            source_kind: "code",
        },
        "cpp" | "cc" | "cxx" | "hpp" | "hh" | "h" | "c" => LanguageDescriptor {
            parser_language: Some("cpp"),
            source_kind: "code",
        },
        "cs" => LanguageDescriptor {
            parser_language: Some("c_sharp"),
            source_kind: "code",
        },
        "rb" => LanguageDescriptor {
            parser_language: Some("ruby"),
            source_kind: "code",
        },
        "php" => LanguageDescriptor {
            parser_language: Some("php"),
            source_kind: "code",
        },
        "swift" => LanguageDescriptor {
            parser_language: Some("swift"),
            source_kind: "code",
        },
        "scala" => LanguageDescriptor {
            parser_language: Some("scala"),
            source_kind: "code",
        },
        "sh" | "bash" => LanguageDescriptor {
            parser_language: Some("bash"),
            source_kind: "config",
        },
        "sql" => LanguageDescriptor {
            parser_language: Some("sql"),
            source_kind: "config",
        },
        "toml" => LanguageDescriptor {
            parser_language: Some("toml"),
            source_kind: "config",
        },
        "yaml" | "yml" => LanguageDescriptor {
            parser_language: Some("yaml"),
            source_kind: "config",
        },
        "json" => LanguageDescriptor {
            parser_language: Some("json"),
            source_kind: "config",
        },
        "md" => LanguageDescriptor {
            parser_language: Some("markdown"),
            source_kind: "docs",
        },
        "txt" | "rst" => LanguageDescriptor {
            parser_language: None,
            source_kind: "docs",
        },
        _ if file_name == "dockerfile" => LanguageDescriptor {
            parser_language: Some("dockerfile"),
            source_kind: "config",
        },
        _ if file_name.ends_with(".env") => LanguageDescriptor {
            parser_language: None,
            source_kind: "config",
        },
        _ => return None,
    };
    Some(desc)
}

#[cfg(test)]
mod tests {
    use super::detect;
    use std::path::Path;

    #[test]
    fn detects_rust() {
        let desc = detect(Path::new("/tmp/example.rs")).unwrap();
        assert_eq!(desc.parser_language, Some("rust"));
        assert_eq!(desc.source_kind, "code");
    }

    #[test]
    fn detects_markdown() {
        let desc = detect(Path::new("/tmp/README.md")).unwrap();
        assert_eq!(desc.parser_language, Some("markdown"));
        assert_eq!(desc.source_kind, "docs");
    }
}
