use crate::config::discover_repo_root;

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

pub(super) fn shell_join_command(args: &[&str]) -> String {
    args.iter()
        .map(|value| shell_quote(value))
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn current_workspace_repo_root_string() -> Option<String> {
    discover_repo_root(None).ok().and_then(|path| {
        path.canonicalize()
            .ok()
            .map(|resolved| resolved.to_string_lossy().to_string())
    })
}

pub(super) fn can_use_workspace_continuity_defaults(
    namespace_code: Option<&str>,
    repo_root: Option<&str>,
) -> bool {
    let Some(repo_root) = repo_root.filter(|value| !value.trim().is_empty()) else {
        return false;
    };
    let Some(current_workspace_repo_root) = current_workspace_repo_root_string() else {
        return false;
    };
    current_workspace_repo_root == repo_root
        && namespace_code
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("continuity")
            == "continuity"
}

pub(super) fn build_workspace_aware_rotate_helper_command(
    project_code: Option<&str>,
    namespace_code: Option<&str>,
    repo_root: Option<&str>,
) -> Option<String> {
    if can_use_workspace_continuity_defaults(namespace_code, repo_root) {
        return Some(shell_join_command(&["amai", "continuity", "rotate-chat"]));
    }
    let project_code = project_code.filter(|value| !value.is_empty())?;
    let namespace_code = namespace_code.filter(|value| !value.is_empty())?;
    let repo_root = repo_root.filter(|value| !value.is_empty())?;
    Some(shell_join_command(&[
        "amai",
        "continuity",
        "rotate-chat",
        "--project",
        project_code,
        "--namespace",
        namespace_code,
        "--repo-root",
        repo_root,
    ]))
}

pub(super) fn build_workspace_aware_startup_command(
    project_code: Option<&str>,
    namespace_code: Option<&str>,
    repo_root: Option<&str>,
    token_source_kind: &str,
    runtime_state_json: bool,
) -> Option<String> {
    let namespace_code = namespace_code.filter(|value| !value.is_empty());
    if can_use_workspace_continuity_defaults(namespace_code, repo_root) {
        let mut args = vec!["amai", "continuity", "startup"];
        if runtime_state_json {
            args.push("--runtime-state-json");
        }
        if !token_source_kind.trim().is_empty()
            && token_source_kind != "operator_continuity_startup"
        {
            args.push("--token-source-kind");
            args.push(token_source_kind);
        }
        return Some(shell_join_command(&args));
    }
    let project_code = project_code.filter(|value| !value.is_empty())?;
    let namespace_code = namespace_code?;
    let repo_root = repo_root.filter(|value| !value.is_empty())?;
    let mut args = vec![
        "amai",
        "continuity",
        "startup",
        "--project",
        project_code,
        "--namespace",
        namespace_code,
        "--repo-root",
        repo_root,
    ];
    if !token_source_kind.trim().is_empty() {
        args.push("--token-source-kind");
        args.push(token_source_kind);
    }
    if runtime_state_json {
        args.push("--runtime-state-json");
    }
    Some(shell_join_command(&args))
}

pub(super) fn build_workspace_aware_handoff_command(
    project_code: Option<&str>,
    namespace_code: Option<&str>,
    repo_root: Option<&str>,
    headline: Option<&str>,
    next_step: Option<&str>,
) -> Option<String> {
    let headline = headline.filter(|value| !value.is_empty())?;
    let next_step = next_step.filter(|value| !value.is_empty())?;
    let namespace_code = namespace_code.filter(|value| !value.is_empty());
    if can_use_workspace_continuity_defaults(namespace_code, repo_root) {
        return Some(shell_join_command(&[
            "./scripts/continuity_handoff.sh",
            "--project",
            "amai",
            "--namespace",
            "continuity",
            "--headline",
            headline,
            "--next-step",
            next_step,
        ]));
    }
    let project_code = project_code.filter(|value| !value.is_empty())?;
    let namespace_code = namespace_code?;
    Some(shell_join_command(&[
        "amai",
        "continuity",
        "handoff",
        "--project",
        project_code,
        "--namespace",
        namespace_code,
        "--headline",
        headline,
        "--next-step",
        next_step,
    ]))
}
