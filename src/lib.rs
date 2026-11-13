//! Yarn / Backstage CLI agent launcher plugin for Basalt.
//!
//! Provides a `CAP_AGENT_LAUNCHER` + `CAP_REVIEW_ACTIONS` plugin that exposes
//! yarn commands as Basalt review actions and an interactive agent session.
//!
//! Review actions are derived from the workspace's root `package.json` so the
//! plugin never advertises a script that does not exist. When no build script
//! is present (for example in a stock Backstage app, which only has `build:all`
//! / `build:backend`), no build action is advertised and Basalt core's native
//! TypeScript check fallback handles the `check` lane.

use basalt_plugin_sdk::prelude::*;
use std::collections::BTreeSet;

basalt_plugin_meta! {
    name:              "backstage-yarn",
    version:           env!("CARGO_PKG_VERSION"),
    hook_flags:        CAP_AGENT_LAUNCHER | CAP_REVIEW_ACTIONS,
    provides:          "agent:backstage-yarn",
    requires:          "",
    optional_requires: "",
    file_globs:        "",
    activates_on:      "package.json\n**/package.json",
    activation_events: "",
}

// ── Review action ids ───────────────────────────────────────────────────────

const ACTION_TEST: &str = "yarn-test";
const ACTION_BUILD: &str = "yarn-build";
const ACTION_LINT_ALL: &str = "yarn-lint-all";
const ACTION_LINT: &str = "yarn-lint";

// ── Host file access ────────────────────────────────────────────────────────
//
// Plugins are compiled for `wasm32-unknown-unknown`, which has no filesystem.
// The host exposes `basalt_read_file` for read-only access instead. The SDK
// does not wrap it, so declare the import directly.

#[cfg(target_arch = "wasm32")]
const MAX_HOST_FILE_BYTES: usize = 256 * 1024;

#[cfg(target_arch = "wasm32")]
fn read_host_file(path: &str) -> Option<Vec<u8>> {
    extern "C" {
        fn basalt_read_file(
            path_ptr: *const u8,
            path_len: i32,
            out_ptr: *mut u8,
            out_cap: i32,
        ) -> i32;
    }

    let mut buf = vec![0u8; MAX_HOST_FILE_BYTES];
    let written = unsafe {
        basalt_read_file(
            path.as_ptr(),
            path.len() as i32,
            buf.as_mut_ptr(),
            buf.len() as i32,
        )
    };
    if written < 0 {
        return None;
    }
    buf.truncate(written as usize);
    Some(buf)
}

#[cfg(not(target_arch = "wasm32"))]
fn read_host_file(path: &str) -> Option<Vec<u8>> {
    std::fs::read(path).ok()
}

fn join_path(root: &str, name: &str) -> String {
    let root = root.trim_end_matches(['/', '\\']);
    if root.is_empty() {
        name.to_string()
    } else {
        format!("{root}/{name}")
    }
}

/// Real workspace root recorded in a shadow workspace's sidecar, if this is a
/// shadow. `review_actions` is called with the shadow workspace by the build
/// runner; the real root is needed for git-specific script checks.
fn real_root_from_shadow(shadow_root: &str) -> Option<String> {
    let raw = read_host_file(&join_path(shadow_root, ".basalt_shadow_meta.json"))?;
    let value: serde_json::Value = serde_json::from_slice(&raw).ok()?;
    value
        .get("workspace_root")
        .and_then(|root| root.as_str())
        .map(str::to_string)
}

fn read_package_scripts(root: &str) -> Option<BTreeSet<String>> {
    let raw = read_host_file(&join_path(root, "package.json"))?;
    let raw: &[u8] = &raw;
    let raw = raw.strip_prefix(&[0xEFu8, 0xBB, 0xBF]).unwrap_or(raw);
    let value: serde_json::Value = serde_json::from_slice(raw).ok()?;
    let scripts = value.get("scripts")?.as_object()?;
    Some(scripts.keys().cloned().collect())
}

/// Read scripts from either of the roots, then from the real root recorded in
/// a shadow sidecar. Returns `None` only when no `package.json` could be read.
fn scripts_from_roots(primary: &str, fallback: &str) -> Option<BTreeSet<String>> {
    for root in [primary, fallback] {
        if let Some(scripts) = read_package_scripts(root) {
            return Some(scripts);
        }
    }
    for root in [primary, fallback] {
        if let Some(real_root) = real_root_from_shadow(root) {
            if let Some(scripts) = read_package_scripts(&real_root) {
                return Some(scripts);
            }
        }
    }
    None
}

fn repo_has_git(root: &str) -> bool {
    if read_host_file(&join_path(root, ".git/HEAD")).is_some() {
        return true;
    }
    real_root_from_shadow(root)
        .is_some_and(|real_root| read_host_file(&join_path(&real_root, ".git/HEAD")).is_some())
}

// ── Agent metadata ──────────────────────────────────────────────────────────

#[basalt_plugin]
pub fn agent_metadata() -> AgentMetadata {
    AgentMetadata {
        name: "Yarn / Backstage CLI".into(),
        executable: "yarn".into(),
        args: vec![],
        resume_new_args: vec!["{prompt}".into()],
        resume_cont_args: vec!["{prompt}".into()],
        execution_tier: AgentExecutionTier::MountedWorkspace,
        workspace_capabilities: vec!["read".into(), "write".into(), "execute".into()],
        protocol: AgentProtocol::Cli,
    }
}

// ── Settings schema ─────────────────────────────────────────────────────────

#[basalt_plugin]
pub fn agent_settings_schema() -> Vec<AgentSettingsField> {
    vec![
        AgentSettingsField {
            kind: AgentSettingsFieldKind::ExecutablePath,
            key: "yarn_executable".into(),
            label: "Yarn executable".into(),
            description: "Path to the yarn binary".into(),
            placeholder: "yarn".into(),
        },
        AgentSettingsField {
            kind: AgentSettingsFieldKind::ExecutablePath,
            key: "node_executable".into(),
            label: "Node executable".into(),
            description: "Path to the node binary".into(),
            placeholder: "node".into(),
        },
    ]
}

// ── Review actions ──────────────────────────────────────────────────────────

#[basalt_plugin]
pub fn review_actions(
    workspace_root: &str,
    session_workspace: &str,
) -> Vec<ReviewActionDescriptor> {
    let Some(scripts) = scripts_from_roots(workspace_root, session_workspace) else {
        return Vec::new();
    };

    let mut actions = Vec::new();

    if scripts.contains("test") {
        actions.push(ReviewActionDescriptor {
            id: ACTION_TEST.into(),
            title: "yarn test (non-interactive)".into(),
            kind: ReviewActionKind::Test,
            ecosystem: "node".into(),
            command_preview: "CI=true yarn test --watchAll=false".into(),
            mutates_workspace: false,
            priority: 10,
        });
    }

    // Only advertise a build action when a literal `build` script exists.
    // Backstage apps use `build:all` / `build:backend`; letting core's
    // `tsc --noEmit` fallback handle `check` is faster and produces
    // structured diagnostics.
    if scripts.contains("build") {
        actions.push(ReviewActionDescriptor {
            id: ACTION_BUILD.into(),
            title: "yarn build".into(),
            kind: ReviewActionKind::Build,
            ecosystem: "node".into(),
            command_preview: "yarn build".into(),
            mutates_workspace: false,
            priority: 20,
        });
    }

    // Prefer `lint:all`: the stock Backstage `lint` script is
    // `backstage-cli repo lint --since origin/<branch>`, which needs a git
    // checkout. Only fall back to `lint` when git is actually available.
    if scripts.contains("lint:all") {
        actions.push(ReviewActionDescriptor {
            id: ACTION_LINT_ALL.into(),
            title: "yarn lint:all".into(),
            kind: ReviewActionKind::Lint,
            ecosystem: "node".into(),
            command_preview: "yarn lint:all".into(),
            mutates_workspace: false,
            priority: 30,
        });
    } else if scripts.contains("lint") && repo_has_git(workspace_root) {
        actions.push(ReviewActionDescriptor {
            id: ACTION_LINT.into(),
            title: "yarn lint".into(),
            kind: ReviewActionKind::Lint,
            ecosystem: "node".into(),
            command_preview: "yarn lint".into(),
            mutates_workspace: false,
            priority: 30,
        });
    }

    actions
}

// ── Review action plan ──────────────────────────────────────────────────────

#[basalt_plugin]
pub fn review_action_plan(
    action_id: &str,
    workspace_root: &str,
    session_workspace: &str,
) -> Option<ReviewActionExecutionPlan> {
    let scripts = scripts_from_roots(workspace_root, session_workspace);
    // If scripts could not be read at plan time, keep the plan: the action was
    // advertised from a successful read moments earlier.
    let script_exists = |name: &str| {
        scripts
            .as_ref()
            .map(|scripts| scripts.contains(name))
            .unwrap_or(true)
    };

    match action_id {
        ACTION_TEST if script_exists("test") => Some(ReviewActionExecutionPlan {
            executable: "yarn".into(),
            args: vec![
                "test".into(),
                "--watchAll=false".into(),
                "--passWithNoTests".into(),
            ],
            env: vec![("CI".into(), "true".into())],
            cwd_mode: ReviewActionCwdMode::SessionWorkspace,
            output_category: "test".into(),
        }),
        ACTION_BUILD if script_exists("build") => Some(ReviewActionExecutionPlan {
            executable: "yarn".into(),
            args: vec!["build".into()],
            env: Vec::new(),
            cwd_mode: ReviewActionCwdMode::SessionWorkspace,
            output_category: "build".into(),
        }),
        ACTION_LINT_ALL if script_exists("lint:all") => Some(ReviewActionExecutionPlan {
            executable: "yarn".into(),
            args: vec!["lint:all".into()],
            env: Vec::new(),
            cwd_mode: ReviewActionCwdMode::SessionWorkspace,
            output_category: "lint".into(),
        }),
        ACTION_LINT if script_exists("lint") => Some(ReviewActionExecutionPlan {
            executable: "yarn".into(),
            args: vec!["lint".into()],
            env: Vec::new(),
            cwd_mode: ReviewActionCwdMode::SessionWorkspace,
            output_category: "lint".into(),
        }),
        _ => None,
    }
}

// ── Output parsing ──────────────────────────────────────────────────────────
//
// Parser state:
//   [counter: u32 LE][last_path_len: u16 LE][last_path bytes]
//
// The counter produces unique vendor ids; `last_path` lets the parser attach
// file names to ESLint's two-line diagnostics.

const MAX_STATE_PATH: usize = 1024;

fn decode_state(state: &[u8]) -> (u32, String) {
    if state.len() < 4 {
        return (0, String::new());
    }
    let counter = u32::from_le_bytes([state[0], state[1], state[2], state[3]]);
    if state.len() < 6 {
        return (counter, String::new());
    }
    let len = u16::from_le_bytes([state[4], state[5]]) as usize;
    let end = (6 + len).min(state.len());
    (
        counter,
        String::from_utf8_lossy(&state[6..end]).into_owned(),
    )
}

fn encode_state(counter: u32, last_path: &str) -> Vec<u8> {
    let bytes = last_path.as_bytes();
    let len = bytes.len().min(MAX_STATE_PATH) as u16;
    let mut out = Vec::with_capacity(6 + len as usize);
    out.extend_from_slice(&counter.to_le_bytes());
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&bytes[..len as usize]);
    out
}

fn next_vendor_id(counter: &mut u32) -> String {
    let id = format!("yarn-{counter}");
    *counter = counter.wrapping_add(1);
    id
}

fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for next in chars.by_ref() {
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(ch);
    }
    out
}

struct ParsedDiagnostic {
    file: String,
    line: u32,
    col: u32,
    severity: &'static str,
    code: Option<String>,
    message: String,
}

fn parse_severity_rest(file: &str, line: u32, col: u32, rest: &str) -> Option<ParsedDiagnostic> {
    let (severity, tail) = if let Some(tail) = rest.strip_prefix("error") {
        ("error", tail)
    } else {
        let tail = rest.strip_prefix("warning")?;
        ("warning", tail)
    };
    // Require a delimiter so "errors" does not count as a diagnostic.
    if !tail.is_empty() && !tail.starts_with(|ch: char| ch == ':' || ch.is_whitespace()) {
        return None;
    }
    let tail = tail.trim_start_matches(|ch: char| ch == ':' || ch.is_whitespace());
    let (code, message) = match tail.split_once(':') {
        Some((code, message)) if !code.is_empty() && code.len() <= 24 && !code.contains(' ') => {
            (Some(code.to_string()), message.trim().to_string())
        }
        _ => (None, tail.to_string()),
    };
    Some(ParsedDiagnostic {
        file: file.to_string(),
        line,
        col,
        severity,
        code,
        message,
    })
}

/// Parse `file(line,col): error TS1234: message` and the pretty
/// `file:line:col - error TS1234: message` form.
fn parse_tsc_diagnostic(line: &str) -> Option<ParsedDiagnostic> {
    let trimmed = line.trim();

    if let Some(open) = trimmed.find('(') {
        if let Some(close_rel) = trimmed[open..].find(')') {
            let close = open + close_rel;
            let coords = &trimmed[open + 1..close];
            let mut parts = coords.split(',');
            let line_no = parts.next()?.trim().parse().ok()?;
            let col_no = parts
                .next()
                .and_then(|col| col.trim().parse().ok())
                .unwrap_or(1);
            let file = trimmed[..open].trim();
            if file.is_empty() {
                return None;
            }
            let rest = trimmed[close + 1..].trim_start_matches(':').trim();
            return parse_severity_rest(file, line_no, col_no, rest);
        }
    }

    let (file, after_file) = trimmed.split_once(':')?;
    if file.is_empty() || file.contains(' ') || file.starts_with("http") {
        return None;
    }
    let (line_str, after_line) = after_file.split_once(':')?;
    let line_no: u32 = line_str.trim().parse().ok()?;
    let digits = after_line
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(after_line.len());
    if digits == 0 {
        return None;
    }
    let col_no: u32 = after_line[..digits].parse().ok()?;
    let rest = after_line[digits..]
        .trim_start_matches(|ch: char| ch == ':' || ch == '-' || ch.is_whitespace())
        .trim();
    parse_severity_rest(file, line_no, col_no, rest)
}

/// Parse ESLint's indented `  12:3  error  message  rule` line, using the
/// file path remembered in parser state.
fn parse_eslint_diagnostic(line: &str, file: &str) -> Option<ParsedDiagnostic> {
    if line.is_empty() || line.len() == line.trim_start().len() || file.is_empty() {
        return None;
    }
    let trimmed = line.trim_start();
    let mut parts = trimmed.split_whitespace();
    let position = parts.next()?;
    let (line_str, col_str) = position.split_once(':')?;
    let line_no: u32 = line_str.parse().ok()?;
    let col_no: u32 = col_str.parse().ok()?;
    let severity = match parts.next()? {
        "error" => "error",
        "warning" => "warning",
        _ => return None,
    };
    let message = parts.collect::<Vec<_>>().join(" ");
    if message.is_empty() {
        return None;
    }
    Some(ParsedDiagnostic {
        file: file.to_string(),
        line: line_no,
        col: col_no,
        severity,
        code: None,
        message,
    })
}

fn looks_like_source_path(line: &str) -> bool {
    const EXTENSIONS: [&str; 10] = [
        ".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs", ".json", ".css", ".scss", ".mts",
    ];
    !line.is_empty()
        && line.len() == line.trim_start().len()
        && !line.contains(':')
        && (line.starts_with('/') || EXTENSIONS.iter().any(|extension| line.ends_with(extension)))
}

fn push_diagnostic(events: &mut Vec<AgentEvent>, counter: &mut u32, diagnostic: ParsedDiagnostic) {
    let file = diagnostic.file;
    let vendor_id = next_vendor_id(counter);
    let raw_cmd = serde_json::json!({
        "file": &file,
        "line": diagnostic.line,
        "col": diagnostic.col,
        "severity": diagnostic.severity,
        "code": diagnostic.code,
        "message": diagnostic.message,
    })
    .to_string();
    events.push(AgentEvent::NewEntry {
        vendor_id,
        tool: if diagnostic.severity == "warning" {
            "Warning".into()
        } else {
            "Error".into()
        },
        category: "diagnostic".into(),
        raw_cmd,
        file_paths: vec![file],
    });
}

fn push_test_result(
    events: &mut Vec<AgentEvent>,
    counter: &mut u32,
    line: &str,
    path: &str,
    passed: bool,
) {
    let vendor_id = next_vendor_id(counter);
    let failure = if passed {
        None
    } else {
        Some(
            serde_json::json!({
                "name": format!("FAIL {path}"),
                "message": line,
                "file": path,
            })
            .to_string(),
        )
    };
    events.push(AgentEvent::NewEntry {
        vendor_id: vendor_id.clone(),
        tool: if passed { "Pass".into() } else { "Fail".into() },
        category: "test".into(),
        raw_cmd: line.to_string(),
        file_paths: vec![path.to_string()],
    });
    events.push(AgentEvent::CloseEntry {
        vendor_id,
        exit_code: if passed { 0 } else { 1 },
        output_lines: failure.into_iter().collect(),
    });
}

fn parse_output_line(
    action_id: Option<&str>,
    line: &[u8],
    state: &[u8],
) -> (Vec<u8>, Vec<AgentEvent>) {
    let (mut counter, mut last_path) = decode_state(state);
    let raw = String::from_utf8_lossy(line);
    let stripped = strip_ansi(raw.as_ref());
    let trimmed = stripped.trim();
    let mut events: Vec<AgentEvent> = Vec::new();

    if trimmed.is_empty() {
        return (encode_state(counter, &last_path), events);
    }

    let diagnostics_enabled = action_id != Some(ACTION_TEST);

    if let Some(path) = trimmed.strip_prefix("PASS ") {
        push_test_result(&mut events, &mut counter, trimmed, path.trim(), true);
    } else if let Some(path) = trimmed.strip_prefix("FAIL ") {
        push_test_result(&mut events, &mut counter, trimmed, path.trim(), false);
    } else if diagnostics_enabled {
        if looks_like_source_path(trimmed) {
            last_path = trimmed.to_string();
        } else if let Some(diagnostic) = parse_tsc_diagnostic(trimmed) {
            push_diagnostic(&mut events, &mut counter, diagnostic);
        } else if let Some(diagnostic) = parse_eslint_diagnostic(&stripped, &last_path) {
            push_diagnostic(&mut events, &mut counter, diagnostic);
        } else if trimmed.starts_with("error ") || trimmed.starts_with("Error:") {
            // Skip yarn's own failure summary; core adds a generic diagnostic
            // for a non-zero exit when the parser produced nothing.
            let is_yarn_summary = trimmed.starts_with("error Command failed")
                || trimmed.starts_with("error See")
                || trimmed.starts_with("error Visit");
            if !is_yarn_summary {
                push_diagnostic(
                    &mut events,
                    &mut counter,
                    ParsedDiagnostic {
                        file: String::new(),
                        line: 1,
                        col: 1,
                        severity: "error",
                        code: None,
                        message: trimmed.to_string(),
                    },
                );
            }
        }
    }

    (encode_state(counter, &last_path), events)
}

#[basalt_plugin]
pub fn review_action_parse_line(
    action_id: &str,
    line: &[u8],
    state: &[u8],
) -> (Vec<u8>, Vec<AgentEvent>) {
    parse_output_line(Some(action_id), line, state)
}

#[basalt_plugin]
pub fn agent_parse_line(line: &[u8], state: &[u8]) -> (Vec<u8>, Vec<AgentEvent>) {
    parse_output_line(None, line, state)
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-time assertions that the generated exports have the arity the
    /// host expects. The old plugin exported `()` / `(i32, i32)` versions and
    /// every call failed silently at runtime.
    #[test]
    fn exports_have_host_compatible_arity() {
        let _: unsafe extern "C" fn(i32, i32, i32, i32) -> u64 = basalt_review_actions;
        let _: unsafe extern "C" fn(i32, i32, i32, i32, i32, i32) -> u64 =
            basalt_review_action_plan;
        let _: unsafe extern "C" fn(i32, i32, i32, i32, i32, i32) -> u64 =
            basalt_review_action_parse_line;
        let _: unsafe extern "C" fn(i32, i32, i32, i32) -> u64 = basalt_agent_parse_line;
        let _: extern "C" fn() -> u64 = basalt_agent_metadata;
        let _: extern "C" fn() -> u64 = basalt_agent_settings_schema;
    }

    fn temp_workspace(name: &str, package_json: Option<&str>) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "basalt-backstage-yarn-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        if let Some(package_json) = package_json {
            std::fs::write(path.join("package.json"), package_json).unwrap();
        }
        path
    }

    const BACKSTAGE_PACKAGE_JSON: &str = r#"{
        "name": "root",
        "private": true,
        "scripts": {
            "build:backend": "yarn workspace backend build",
            "build:all": "backstage-cli repo build --all",
            "tsc": "tsc",
            "test": "backstage-cli repo test",
            "lint": "backstage-cli repo lint --since origin/master",
            "lint:all": "backstage-cli repo lint"
        }
    }"#;

    fn action_ids(actions: &[ReviewActionDescriptor]) -> Vec<&str> {
        actions.iter().map(|action| action.id.as_str()).collect()
    }

    #[test]
    fn review_actions_only_advertise_existing_scripts() {
        let workspace = temp_workspace("backstage", Some(BACKSTAGE_PACKAGE_JSON));
        let root = workspace.to_string_lossy();

        let actions = review_actions(&root, &root);
        assert_eq!(action_ids(&actions), vec![ACTION_TEST, ACTION_LINT_ALL]);

        let test = &actions[0];
        assert_eq!(test.kind, ReviewActionKind::Test);
        assert_eq!(test.priority, 10);
        assert!(test.command_preview.contains("--watchAll=false"));

        // Stock Backstage has no `build` script: the native tsc fallback is
        // used for the check lane instead of advertising a broken action.
        assert!(actions.iter().all(|action| action.id != ACTION_BUILD));

        let _ = std::fs::remove_dir_all(&workspace);
    }

    #[test]
    fn review_actions_advertise_build_when_script_exists() {
        let workspace = temp_workspace(
            "build",
            Some(r#"{"scripts": {"test": "jest", "build": "tsc -b"}}"#),
        );
        let root = workspace.to_string_lossy();

        let actions = review_actions(&root, &root);
        assert_eq!(action_ids(&actions), vec![ACTION_TEST, ACTION_BUILD]);
        assert_eq!(actions[1].kind, ReviewActionKind::Build);

        let _ = std::fs::remove_dir_all(&workspace);
    }

    #[test]
    fn review_actions_skip_lint_without_git_or_lint_all() {
        let workspace = temp_workspace(
            "lint",
            Some(r#"{"scripts": {"test": "jest", "lint": "eslint ."}}"#),
        );
        let root = workspace.to_string_lossy();

        let actions = review_actions(&root, &root);
        assert_eq!(action_ids(&actions), vec![ACTION_TEST]);

        let _ = std::fs::remove_dir_all(&workspace);
    }

    #[test]
    fn review_actions_are_empty_without_package_json() {
        let workspace = temp_workspace("empty", None);
        let root = workspace.to_string_lossy();

        assert!(review_actions(&root, &root).is_empty());

        let _ = std::fs::remove_dir_all(&workspace);
    }

    #[test]
    fn test_plan_is_non_interactive() {
        let workspace = temp_workspace("plan", Some(BACKSTAGE_PACKAGE_JSON));
        let root = workspace.to_string_lossy();

        let plan = review_action_plan(ACTION_TEST, &root, &root).unwrap();
        assert_eq!(plan.executable, "yarn");
        assert_eq!(
            plan.args,
            vec!["test", "--watchAll=false", "--passWithNoTests"]
        );
        assert_eq!(plan.env, vec![("CI".to_string(), "true".to_string())]);
        assert_eq!(plan.cwd_mode, ReviewActionCwdMode::SessionWorkspace);

        let _ = std::fs::remove_dir_all(&workspace);
    }

    #[test]
    fn unknown_action_has_no_plan() {
        let workspace = temp_workspace("unknown", Some(BACKSTAGE_PACKAGE_JSON));
        let root = workspace.to_string_lossy();

        assert!(review_action_plan("nope", &root, &root).is_none());

        let _ = std::fs::remove_dir_all(&workspace);
    }

    #[test]
    fn review_action_parse_line_reports_pass_and_fail() {
        let (_, events) = review_action_parse_line(ACTION_TEST, b"PASS src/foo.test.ts", &[]);
        assert_eq!(events.len(), 2);
        match &events[1] {
            AgentEvent::CloseEntry {
                exit_code,
                output_lines,
                ..
            } => {
                assert_eq!(*exit_code, 0);
                assert!(output_lines.is_empty());
            }
            other => panic!("expected CloseEntry, got {other:?}"),
        }

        let (_, events) = review_action_parse_line(ACTION_TEST, b"FAIL src/bar.test.ts", &[]);
        match &events[1] {
            AgentEvent::CloseEntry {
                exit_code,
                output_lines,
                ..
            } => {
                assert_eq!(*exit_code, 1);
                let failure: serde_json::Value = serde_json::from_str(&output_lines[0]).unwrap();
                assert_eq!(failure["file"], "src/bar.test.ts");
            }
            other => panic!("expected CloseEntry, got {other:?}"),
        }
    }

    #[test]
    fn review_action_parse_line_emits_tsc_and_eslint_diagnostics() {
        let (_, events) = review_action_parse_line(
            ACTION_BUILD,
            b"src/foo.ts(12,3): error TS2322: Type 'string' is not assignable",
            &[],
        );
        match &events[0] {
            AgentEvent::NewEntry {
                category, raw_cmd, ..
            } => {
                assert_eq!(category, "diagnostic");
                let diagnostic: serde_json::Value = serde_json::from_str(raw_cmd).unwrap();
                assert_eq!(diagnostic["file"], "src/foo.ts");
                assert_eq!(diagnostic["line"], 12);
                assert_eq!(diagnostic["col"], 3);
                assert_eq!(diagnostic["severity"], "error");
                assert_eq!(diagnostic["code"], "TS2322");
            }
            other => panic!("expected NewEntry, got {other:?}"),
        }

        // ESLint prints the file on one line, then diagnostics on the next.
        let (state, events) = review_action_parse_line(ACTION_LINT_ALL, b"src/foo.ts", &[]);
        assert!(events.is_empty());

        let (_, events) = review_action_parse_line(
            ACTION_LINT_ALL,
            b"  4:7  warning  Unexpected any  @typescript-eslint/no-explicit-any",
            &state,
        );
        match &events[0] {
            AgentEvent::NewEntry { raw_cmd, .. } => {
                let diagnostic: serde_json::Value = serde_json::from_str(raw_cmd).unwrap();
                assert_eq!(diagnostic["file"], "src/foo.ts");
                assert_eq!(diagnostic["line"], 4);
                assert_eq!(diagnostic["col"], 7);
                assert_eq!(diagnostic["severity"], "warning");
            }
            other => panic!("expected NewEntry, got {other:?}"),
        }
    }

    #[test]
    fn agent_parse_line_takes_line_before_state() {
        // The host calls `basalt_agent_parse_line(line, state)`. Passing the
        // line first must produce a parsed event; the old reversed signature
        // would treat the 0-byte state as the line and emit nothing.
        let (_, events) = agent_parse_line(b"FAIL src/baz.test.ts", &[]);
        assert_eq!(events.len(), 2);
        match &events[1] {
            AgentEvent::CloseEntry { exit_code, .. } => assert_eq!(*exit_code, 1),
            other => panic!("expected CloseEntry, got {other:?}"),
        }
    }

    #[test]
    fn parser_state_carries_counter_and_file() {
        let (state, _) = review_action_parse_line(ACTION_TEST, b"PASS a.test.ts", &[]);
        let (state, events) = review_action_parse_line(ACTION_TEST, b"FAIL b.test.ts", &state);
        match &events[0] {
            AgentEvent::NewEntry { vendor_id, .. } => assert_eq!(vendor_id, "yarn-1"),
            other => panic!("expected NewEntry, got {other:?}"),
        }
        let (counter, _) = decode_state(&state);
        assert_eq!(counter, 2);
    }
}
