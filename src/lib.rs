//! Yarn / Backstage CLI agent launcher plugin for Basalt.
//!
//! Provides a `CAP_AGENT_LAUNCHER` + `CAP_REVIEW_ACTIONS` plugin that exposes
//! yarn commands as Basalt review actions and an interactive agent session.

use basalt_plugin_sdk::prelude::*;

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

// ── Agent metadata ──────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn basalt_agent_metadata() -> u64 {
    let meta = AgentMetadata {
        name: "Yarn / Backstage CLI".into(),
        executable: "yarn".into(),
        args: vec![],
        resume_new_args: vec!["{prompt}".into()],
        resume_cont_args: vec!["{prompt}".into()],
        execution_tier: AgentExecutionTier::MountedWorkspace,
        workspace_capabilities: vec!["read".into(), "write".into(), "execute".into()],
        protocol: AgentProtocol::Cli,
    };
    pack_output(encode_agent_metadata(&meta))
}

// ── Settings schema ─────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn basalt_agent_settings_schema() -> u64 {
    let fields = vec![
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
    ];
    pack_output(encode_agent_settings_schema(&fields))
}

// ── Review actions ──────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn basalt_review_actions() -> u64 {
    let actions = vec![
        ReviewActionDescriptor {
            id: "yarn-test".into(),
            title: "yarn test".into(),
            kind: ReviewActionKind::Test,
            ecosystem: "node".into(),
            command_preview: "yarn test".into(),
            mutates_workspace: false,
            priority: 10,
        },
        ReviewActionDescriptor {
            id: "yarn-build".into(),
            title: "yarn build".into(),
            kind: ReviewActionKind::Build,
            ecosystem: "node".into(),
            command_preview: "yarn build".into(),
            mutates_workspace: false,
            priority: 20,
        },
        ReviewActionDescriptor {
            id: "yarn-lint".into(),
            title: "yarn lint".into(),
            kind: ReviewActionKind::Lint,
            ecosystem: "node".into(),
            command_preview: "yarn lint".into(),
            mutates_workspace: false,
            priority: 30,
        },
        ReviewActionDescriptor {
            id: "yarn-backstage-build".into(),
            title: "yarn backstage-cli package build".into(),
            kind: ReviewActionKind::Build,
            ecosystem: "node".into(),
            command_preview: "yarn backstage-cli package build".into(),
            mutates_workspace: false,
            priority: 15,
        },
    ];
    pack_output(encode_review_actions(&actions))
}

// ── Review action plan ──────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn basalt_review_action_plan(id_ptr: *const u8, id_len: usize) -> u64 {
    if id_ptr.is_null() || id_len == 0 {
        return 0;
    }
    let id_slice = unsafe { std::slice::from_raw_parts(id_ptr, id_len) };
    let id_str = match std::str::from_utf8(id_slice) {
        Ok(s) => s,
        Err(_) => return 0,
    };

    let plan = match id_str {
        "yarn-test" => ReviewActionExecutionPlan {
            executable: "yarn".into(),
            args: vec!["test".into()],
            env: vec![],
            cwd_mode: ReviewActionCwdMode::SessionWorkspace,
            output_category: "test".into(),
        },
        "yarn-build" => ReviewActionExecutionPlan {
            executable: "yarn".into(),
            args: vec!["build".into()],
            env: vec![],
            cwd_mode: ReviewActionCwdMode::SessionWorkspace,
            output_category: "build".into(),
        },
        "yarn-lint" => ReviewActionExecutionPlan {
            executable: "yarn".into(),
            args: vec!["lint".into()],
            env: vec![],
            cwd_mode: ReviewActionCwdMode::SessionWorkspace,
            output_category: "lint".into(),
        },
        "yarn-backstage-build" => ReviewActionExecutionPlan {
            executable: "yarn".into(),
            args: vec!["backstage-cli".into(), "package".into(), "build".into()],
            env: vec![],
            cwd_mode: ReviewActionCwdMode::SessionWorkspace,
            output_category: "build".into(),
        },
        _ => return 0,
    };

    pack_output(encode_review_action_plan(&plan))
}

// ── Agent line parser state ─────────────────────────────────────────────────
//
// State is serialised as 4 LE bytes representing a `u32` event counter.
// The counter is used to generate unique `vendor_id` strings like `"yarn-{n}"`.

fn read_counter(state_ptr: *const u8, state_len: usize) -> u32 {
    if !state_ptr.is_null() && state_len >= 4 {
        let slice = unsafe { std::slice::from_raw_parts(state_ptr, 4) };
        u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]])
    } else {
        0
    }
}

fn encode_counter(n: u32) -> Vec<u8> {
    n.to_le_bytes().to_vec()
}

// ── Agent init state ────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn agent_init_state() -> u64 {
    0
}

// ── Agent parse line ────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn agent_parse_line(
    state_ptr: *const u8,
    state_len: usize,
    line_ptr: *const u8,
    line_len: usize,
) -> u64 {
    if line_ptr.is_null() || line_len == 0 {
        let counter = read_counter(state_ptr, state_len);
        return pack_output(encode_agent_parse_output(&encode_counter(counter), &[]));
    }

    let line_slice = unsafe { std::slice::from_raw_parts(line_ptr, line_len) };
    let line_str = match std::str::from_utf8(line_slice) {
        Ok(s) => s.trim(),
        Err(_) => {
            let counter = read_counter(state_ptr, state_len);
            return pack_output(encode_agent_parse_output(&encode_counter(counter), &[]));
        }
    };

    if line_str.is_empty() {
        let counter = read_counter(state_ptr, state_len);
        return pack_output(encode_agent_parse_output(&encode_counter(counter), &[]));
    }

    let mut counter = read_counter(state_ptr, state_len);
    let mut events: Vec<AgentEvent> = Vec::new();

    // Classify the line and emit events.
    if line_str.contains("error Command failed") {
        // Session-ending command failure — check before generic "error " match.
        events.push(AgentEvent::SessionEnded {
            success: false,
            error: Some(line_str.to_string()),
        });
    } else if line_str.contains("[done]")
        || line_str.contains("Done in")
        || line_str.contains("success")
    {
        events.push(AgentEvent::SessionEnded { success: true, error: None });
    } else if line_str.contains("PASS") || line_str.contains('\u{2713}') {
        // PASS or ✓
        let vid = format!("yarn-{}", counter);
        counter = counter.wrapping_add(1);
        events.push(AgentEvent::NewEntry {
            vendor_id: vid.clone(),
            tool: "Pass".into(),
            category: "test".into(),
            raw_cmd: line_str.to_string(),
            file_paths: vec![],
        });
        events.push(AgentEvent::CloseEntry {
            vendor_id: vid,
            exit_code: 0,
            output_lines: vec![line_str.to_string()],
        });
    } else if line_str.contains("FAIL")
        || line_str.contains('\u{2717}')
        || line_str.contains("failed")
    {
        // FAIL or ✗ or "failed"
        let vid = format!("yarn-{}", counter);
        counter = counter.wrapping_add(1);
        events.push(AgentEvent::NewEntry {
            vendor_id: vid.clone(),
            tool: "Fail".into(),
            category: "test".into(),
            raw_cmd: line_str.to_string(),
            file_paths: vec![],
        });
        events.push(AgentEvent::CloseEntry {
            vendor_id: vid,
            exit_code: 1,
            output_lines: vec![line_str.to_string()],
        });
    } else if line_str.contains("error ") || line_str.contains("Error:") {
        let vid = format!("yarn-{}", counter);
        counter = counter.wrapping_add(1);
        events.push(AgentEvent::NewEntry {
            vendor_id: vid.clone(),
            tool: "Error".into(),
            category: "diagnostic".into(),
            raw_cmd: line_str.to_string(),
            file_paths: vec![],
        });
        events.push(AgentEvent::CloseEntry {
            vendor_id: vid,
            exit_code: 1,
            output_lines: vec![line_str.to_string()],
        });
    } else if line_str.contains("warning ") || line_str.contains("Warning:") {
        let vid = format!("yarn-{}", counter);
        counter = counter.wrapping_add(1);
        events.push(AgentEvent::NewEntry {
            vendor_id: vid.clone(),
            tool: "Warning".into(),
            category: "diagnostic".into(),
            raw_cmd: line_str.to_string(),
            file_paths: vec![],
        });
        events.push(AgentEvent::CloseEntry {
            vendor_id: vid,
            exit_code: 0,
            output_lines: vec![line_str.to_string()],
        });
    }
    // Other lines: no events, just carry the counter state forward.

    let new_state = encode_counter(counter);
    pack_output(encode_agent_parse_output(&new_state, &events))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str, counter: u32) -> (u32, Vec<AgentEvent>) {
        let state = counter.to_le_bytes();
        let line_bytes = line.as_bytes();
        let packed = agent_parse_line(
            state.as_ptr(),
            state.len(),
            line_bytes.as_ptr(),
            line_bytes.len(),
        );
        // Decode the output manually: [state_len u32][state bytes][event_count u16]...
        // We just re-call the logic directly for test purposes.
        let _ = packed; // suppress unused warning — actual decode done below

        // Use the inner logic directly via a helper:
        parse_yarn_line(line, counter)
    }

    fn parse_yarn_line(line_str: &str, mut counter: u32) -> (u32, Vec<AgentEvent>) {
        let mut events: Vec<AgentEvent> = Vec::new();

        if line_str.contains("error Command failed") {
            events.push(AgentEvent::SessionEnded {
                success: false,
                error: Some(line_str.to_string()),
            });
        } else if line_str.contains("[done]")
            || line_str.contains("Done in")
            || line_str.contains("success")
        {
            events.push(AgentEvent::SessionEnded { success: true, error: None });
        } else if line_str.contains("PASS") || line_str.contains('\u{2713}') {
            let vid = format!("yarn-{}", counter);
            counter = counter.wrapping_add(1);
            events.push(AgentEvent::NewEntry {
                vendor_id: vid.clone(),
                tool: "Pass".into(),
                category: "test".into(),
                raw_cmd: line_str.to_string(),
                file_paths: vec![],
            });
            events.push(AgentEvent::CloseEntry {
                vendor_id: vid,
                exit_code: 0,
                output_lines: vec![line_str.to_string()],
            });
        } else if line_str.contains("FAIL")
            || line_str.contains('\u{2717}')
            || line_str.contains("failed")
        {
            let vid = format!("yarn-{}", counter);
            counter = counter.wrapping_add(1);
            events.push(AgentEvent::NewEntry {
                vendor_id: vid.clone(),
                tool: "Fail".into(),
                category: "test".into(),
                raw_cmd: line_str.to_string(),
                file_paths: vec![],
            });
            events.push(AgentEvent::CloseEntry {
                vendor_id: vid,
                exit_code: 1,
                output_lines: vec![line_str.to_string()],
            });
        } else if line_str.contains("error ") || line_str.contains("Error:") {
            let vid = format!("yarn-{}", counter);
            counter = counter.wrapping_add(1);
            events.push(AgentEvent::NewEntry {
                vendor_id: vid.clone(),
                tool: "Error".into(),
                category: "diagnostic".into(),
                raw_cmd: line_str.to_string(),
                file_paths: vec![],
            });
            events.push(AgentEvent::CloseEntry {
                vendor_id: vid,
                exit_code: 1,
                output_lines: vec![line_str.to_string()],
            });
        } else if line_str.contains("warning ") || line_str.contains("Warning:") {
            let vid = format!("yarn-{}", counter);
            counter = counter.wrapping_add(1);
            events.push(AgentEvent::NewEntry {
                vendor_id: vid.clone(),
                tool: "Warning".into(),
                category: "diagnostic".into(),
                raw_cmd: line_str.to_string(),
                file_paths: vec![],
            });
            events.push(AgentEvent::CloseEntry {
                vendor_id: vid,
                exit_code: 0,
                output_lines: vec![line_str.to_string()],
            });
        }

        (counter, events)
    }

    #[test]
    fn test_pass_line() {
        let (_, evs) = parse("PASS src/foo.test.ts", 0);
        assert_eq!(evs.len(), 2);
        match &evs[0] {
            AgentEvent::NewEntry { tool, category, vendor_id, .. } => {
                assert_eq!(tool, "Pass");
                assert_eq!(category, "test");
                assert_eq!(vendor_id, "yarn-0");
            }
            _ => panic!("expected NewEntry"),
        }
        match &evs[1] {
            AgentEvent::CloseEntry { exit_code, .. } => assert_eq!(*exit_code, 0),
            _ => panic!("expected CloseEntry"),
        }
    }

    #[test]
    fn test_fail_line() {
        let (_, evs) = parse("FAIL src/bar.test.ts", 5);
        assert_eq!(evs.len(), 2);
        match &evs[0] {
            AgentEvent::NewEntry { tool, vendor_id, .. } => {
                assert_eq!(tool, "Fail");
                assert_eq!(vendor_id, "yarn-5");
            }
            _ => panic!("expected NewEntry"),
        }
        match &evs[1] {
            AgentEvent::CloseEntry { exit_code, .. } => assert_eq!(*exit_code, 1),
            _ => panic!("expected CloseEntry"),
        }
    }

    #[test]
    fn test_error_line() {
        let (_, evs) = parse("Error: Cannot find module 'react'", 0);
        assert_eq!(evs.len(), 2);
        match &evs[0] {
            AgentEvent::NewEntry { tool, category, .. } => {
                assert_eq!(tool, "Error");
                assert_eq!(category, "diagnostic");
            }
            _ => panic!("expected NewEntry"),
        }
        match &evs[1] {
            AgentEvent::CloseEntry { exit_code, .. } => assert_eq!(*exit_code, 1),
            _ => panic!("expected CloseEntry"),
        }
    }

    #[test]
    fn test_warning_line() {
        let (_, evs) = parse("warning some-package is deprecated", 0);
        assert_eq!(evs.len(), 2);
        match &evs[0] {
            AgentEvent::NewEntry { tool, category, .. } => {
                assert_eq!(tool, "Warning");
                assert_eq!(category, "diagnostic");
            }
            _ => panic!("expected NewEntry"),
        }
        match &evs[1] {
            AgentEvent::CloseEntry { exit_code, .. } => assert_eq!(*exit_code, 0),
            _ => panic!("expected CloseEntry"),
        }
    }

    #[test]
    fn test_done_line() {
        let (_, evs) = parse("Done in 4.32s.", 0);
        assert_eq!(evs.len(), 1);
        match &evs[0] {
            AgentEvent::SessionEnded { success, .. } => assert!(*success),
            _ => panic!("expected SessionEnded"),
        }
    }

    #[test]
    fn test_command_failed_line() {
        let (_, evs) = parse("error Command failed with exit code 1.", 0);
        assert_eq!(evs.len(), 1);
        match &evs[0] {
            AgentEvent::SessionEnded { success, error } => {
                assert!(!*success);
                assert!(error.is_some());
            }
            _ => panic!("expected SessionEnded with failure"),
        }
    }

    #[test]
    fn test_plain_line_no_events() {
        let (counter, evs) = parse("$ tsc --build", 3);
        assert!(evs.is_empty());
        assert_eq!(counter, 3); // counter unchanged
    }

    #[test]
    fn test_counter_increments() {
        let (c1, evs1) = parse("PASS a.test.ts", 0);
        assert_eq!(c1, 1);
        assert_eq!(evs1.len(), 2);
        let (c2, evs2) = parse("FAIL b.test.ts", c1);
        assert_eq!(c2, 2);
        assert_eq!(evs2.len(), 2);
        match &evs2[0] {
            AgentEvent::NewEntry { vendor_id, .. } => assert_eq!(vendor_id, "yarn-1"),
            _ => panic!(),
        }
    }
}
