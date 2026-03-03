//! Integration tests for the MCP server module.
//!
//! Tests the MCP protocol types, tool definitions, tool handlers,
//! resource definitions, resource handlers, and the server dispatch loop.

use std::fs;

use serde_json::{json, Value};
use tempfile::TempDir;

use bn::bean::Bean;
use bn::index::Index;
use bn::mcp::protocol::{JsonRpcRequest, JsonRpcResponse};
use bn::mcp::resources;
use bn::mcp::tools;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Create a temporary .beans/ directory with config and sample beans.
fn setup_mcp_env() -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().unwrap();
    let beans_dir = dir.path().join(".beans");
    fs::create_dir_all(&beans_dir).unwrap();

    // Write minimal config YAML — serde defaults handle the rest
    fs::write(
        beans_dir.join("config.yaml"),
        "project: mcp-test\nnext_id: 4\n",
    )
    .unwrap();

    // Bean 1: open with verify (scoped with produces/paths)
    let mut bean1 = Bean::new("1", "Fix login bug");
    bean1.slug = Some("fix-login-bug".to_string());
    bean1.verify = Some("echo pass".to_string());
    bean1.description = Some("Fix the login authentication flow".to_string());
    bean1.produces = vec!["LoginFix".to_string()];
    bean1.paths = vec!["src/login.rs".to_string()];
    bean1.to_file(beans_dir.join("1-fix-login-bug.md")).unwrap();

    // Bean 2: open, depends on 1 (scoped with produces/paths)
    let mut bean2 = Bean::new("2", "Add tests for login");
    bean2.slug = Some("add-tests-for-login".to_string());
    bean2.verify = Some("echo pass".to_string());
    bean2.dependencies = vec!["1".to_string()];
    bean2.produces = vec!["LoginTests".to_string()];
    bean2.paths = vec!["tests/login.rs".to_string()];
    bean2
        .to_file(beans_dir.join("2-add-tests-for-login.md"))
        .unwrap();

    // Bean 3: open goal (no verify, but scoped)
    let mut bean3 = Bean::new("3", "Refactor auth module");
    bean3.slug = Some("refactor-auth-module".to_string());
    bean3.priority = 1;
    bean3.produces = vec!["AuthRefactor".to_string()];
    bean3.paths = vec!["src/auth.rs".to_string()];
    bean3
        .to_file(beans_dir.join("3-refactor-auth-module.md"))
        .unwrap();

    // Build index
    let index = Index::build(&beans_dir).unwrap();
    index.save(&beans_dir).unwrap();

    (dir, beans_dir)
}

// ---------------------------------------------------------------------------
// Protocol types
// ---------------------------------------------------------------------------

#[test]
fn mcp_json_rpc_request_deserializes() {
    let json_str = r#"{"jsonrpc":"2.0","method":"initialize","id":1,"params":{"protocolVersion":"2024-11-05"}}"#;
    let req: JsonRpcRequest = serde_json::from_str(json_str).unwrap();
    assert_eq!(req.method, "initialize");
    assert_eq!(req.id, Some(json!(1)));
    assert!(req.params.is_some());
}

#[test]
fn mcp_json_rpc_request_without_id_is_notification() {
    let json_str = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
    let req: JsonRpcRequest = serde_json::from_str(json_str).unwrap();
    assert_eq!(req.method, "notifications/initialized");
    assert!(req.id.is_none());
}

#[test]
fn mcp_json_rpc_response_success_serializes() {
    let resp = JsonRpcResponse::success(json!(1), json!({"status": "ok"}));
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["jsonrpc"], "2.0");
    assert_eq!(json["id"], 1);
    assert_eq!(json["result"]["status"], "ok");
    assert!(json.get("error").is_none());
}

#[test]
fn mcp_json_rpc_response_error_serializes() {
    let resp = JsonRpcResponse::error(json!(1), -32601, "Method not found");
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["jsonrpc"], "2.0");
    assert_eq!(json["id"], 1);
    assert_eq!(json["error"]["code"], -32601);
    assert_eq!(json["error"]["message"], "Method not found");
    assert!(json.get("result").is_none());
}

// ---------------------------------------------------------------------------
// Tool definitions
// ---------------------------------------------------------------------------

#[test]
fn mcp_tool_definitions_returns_all_ten_tools() {
    let defs = tools::tool_definitions();
    assert_eq!(defs.len(), 10, "Expected 10 tools, got {}", defs.len());

    let names: Vec<&str> = defs.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"list_beans"));
    assert!(names.contains(&"show_bean"));
    assert!(names.contains(&"ready_beans"));
    assert!(names.contains(&"create_bean"));
    assert!(names.contains(&"claim_bean"));
    assert!(names.contains(&"close_bean"));
    assert!(names.contains(&"verify_bean"));
    assert!(names.contains(&"context_bean"));
    assert!(names.contains(&"status"));
    assert!(names.contains(&"tree"));
}

#[test]
fn mcp_tool_definitions_have_valid_json_schemas() {
    let defs = tools::tool_definitions();
    for tool in &defs {
        assert!(!tool.name.is_empty(), "Tool name should not be empty");
        assert!(
            !tool.description.is_empty(),
            "Tool {} description should not be empty",
            tool.name
        );
        // input_schema must be an object with "type": "object"
        assert_eq!(
            tool.input_schema["type"], "object",
            "Tool {} input_schema must have type: object",
            tool.name
        );
    }
}

#[test]
fn mcp_required_tools_have_required_params() {
    let defs = tools::tool_definitions();

    let show = defs.iter().find(|t| t.name == "show_bean").unwrap();
    let required = show.input_schema["required"].as_array().unwrap();
    assert!(required.contains(&json!("id")));

    let create = defs.iter().find(|t| t.name == "create_bean").unwrap();
    let required = create.input_schema["required"].as_array().unwrap();
    assert!(required.contains(&json!("title")));
}

// ---------------------------------------------------------------------------
// Tool handlers: list_beans
// ---------------------------------------------------------------------------

#[test]
fn mcp_list_beans_returns_all_open() {
    let (_dir, beans_dir) = setup_mcp_env();
    let result = tools::handle_tool_call("list_beans", &json!({}), &beans_dir);

    let text = result["content"][0]["text"].as_str().unwrap();
    let parsed: Value = serde_json::from_str(text).unwrap();
    assert_eq!(parsed["count"], 3);
}

#[test]
fn mcp_list_beans_filter_by_priority() {
    let (_dir, beans_dir) = setup_mcp_env();
    let result = tools::handle_tool_call("list_beans", &json!({"priority": 1}), &beans_dir);

    let text = result["content"][0]["text"].as_str().unwrap();
    let parsed: Value = serde_json::from_str(text).unwrap();
    assert_eq!(parsed["count"], 1);
    assert_eq!(parsed["beans"][0]["title"], "Refactor auth module");
}

// ---------------------------------------------------------------------------
// Tool handlers: show_bean
// ---------------------------------------------------------------------------

#[test]
fn mcp_show_bean_returns_full_details() {
    let (_dir, beans_dir) = setup_mcp_env();
    let result = tools::handle_tool_call("show_bean", &json!({"id": "1"}), &beans_dir);

    assert!(result.get("isError").is_none());
    let text = result["content"][0]["text"].as_str().unwrap();
    let parsed: Value = serde_json::from_str(text).unwrap();
    assert_eq!(parsed["id"], "1");
    assert_eq!(parsed["title"], "Fix login bug");
    assert_eq!(parsed["verify"], "echo pass");
}

#[test]
fn mcp_show_bean_missing_id_returns_error() {
    let (_dir, beans_dir) = setup_mcp_env();
    let result = tools::handle_tool_call("show_bean", &json!({}), &beans_dir);

    assert_eq!(result["isError"], true);
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Missing required parameter: id"));
}

#[test]
fn mcp_show_bean_invalid_id_returns_error() {
    let (_dir, beans_dir) = setup_mcp_env();
    let result = tools::handle_tool_call("show_bean", &json!({"id": "999"}), &beans_dir);

    assert_eq!(result["isError"], true);
}

// ---------------------------------------------------------------------------
// Tool handlers: ready_beans
// ---------------------------------------------------------------------------

#[test]
fn mcp_ready_beans_excludes_blocked() {
    let (_dir, beans_dir) = setup_mcp_env();
    let result = tools::handle_tool_call("ready_beans", &json!({}), &beans_dir);

    let text = result["content"][0]["text"].as_str().unwrap();
    let parsed: Value = serde_json::from_str(text).unwrap();

    // Bean 1 is ready (has verify, no deps)
    // Bean 2 is blocked (depends on 1)
    // Bean 3 has no verify
    assert_eq!(parsed["count"], 1);
    assert_eq!(parsed["ready"][0]["id"], "1");
}

// ---------------------------------------------------------------------------
// Tool handlers: create_bean
// ---------------------------------------------------------------------------

#[test]
fn mcp_create_bean_basic() {
    let (_dir, beans_dir) = setup_mcp_env();
    let result = tools::handle_tool_call(
        "create_bean",
        &json!({
            "title": "New task from MCP",
            "verify": "echo test",
            "description": "Created via MCP"
        }),
        &beans_dir,
    );

    assert!(result.get("isError").is_none());
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Created bean 4"));
    assert!(text.contains("New task from MCP"));

    // Verify bean was actually written
    let index = Index::load_or_rebuild(&beans_dir).unwrap();
    let entry = index.beans.iter().find(|e| e.id == "4").unwrap();
    assert_eq!(entry.title, "New task from MCP");
    assert!(entry.has_verify);
}

#[test]
fn mcp_create_bean_missing_title_returns_error() {
    let (_dir, beans_dir) = setup_mcp_env();
    let result = tools::handle_tool_call("create_bean", &json!({}), &beans_dir);

    assert_eq!(result["isError"], true);
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Missing required parameter: title"));
}

#[test]
fn mcp_create_bean_with_priority() {
    let (_dir, beans_dir) = setup_mcp_env();
    let result = tools::handle_tool_call(
        "create_bean",
        &json!({
            "title": "Urgent fix",
            "priority": 0
        }),
        &beans_dir,
    );

    assert!(result.get("isError").is_none());

    let index = Index::load_or_rebuild(&beans_dir).unwrap();
    let entry = index.beans.iter().find(|e| e.id == "4").unwrap();
    assert_eq!(entry.priority, 0);
}

// ---------------------------------------------------------------------------
// Tool handlers: claim_bean
// ---------------------------------------------------------------------------

#[test]
fn mcp_claim_bean_sets_in_progress() {
    let (_dir, beans_dir) = setup_mcp_env();
    let result = tools::handle_tool_call(
        "claim_bean",
        &json!({"id": "1", "by": "cursor-agent"}),
        &beans_dir,
    );

    assert!(result.get("isError").is_none());
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Claimed bean 1"));
    assert!(text.contains("cursor-agent"));

    // Verify status changed
    let index = Index::load_or_rebuild(&beans_dir).unwrap();
    let entry = index.beans.iter().find(|e| e.id == "1").unwrap();
    assert_eq!(format!("{}", entry.status), "in_progress");
}

#[test]
fn mcp_claim_bean_already_claimed_returns_error() {
    let (_dir, beans_dir) = setup_mcp_env();

    // Claim once
    tools::handle_tool_call(
        "claim_bean",
        &json!({"id": "1", "by": "agent-1"}),
        &beans_dir,
    );

    // Claim again — should fail
    let result = tools::handle_tool_call(
        "claim_bean",
        &json!({"id": "1", "by": "agent-2"}),
        &beans_dir,
    );

    assert_eq!(result["isError"], true);
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("in_progress"));
}

// ---------------------------------------------------------------------------
// Tool handlers: verify_bean
// ---------------------------------------------------------------------------

#[test]
fn mcp_verify_bean_passing() {
    let (_dir, beans_dir) = setup_mcp_env();
    let result = tools::handle_tool_call("verify_bean", &json!({"id": "1"}), &beans_dir);

    assert!(result.get("isError").is_none());
    let text = result["content"][0]["text"].as_str().unwrap();
    let parsed: Value = serde_json::from_str(text).unwrap();
    assert_eq!(parsed["passed"], true);
    assert_eq!(parsed["command"], "echo pass");
}

#[test]
fn mcp_verify_bean_no_verify_command() {
    let (_dir, beans_dir) = setup_mcp_env();
    let result = tools::handle_tool_call("verify_bean", &json!({"id": "3"}), &beans_dir);

    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("no verify command"));
}

// ---------------------------------------------------------------------------
// Tool handlers: close_bean
// ---------------------------------------------------------------------------

#[test]
fn mcp_close_bean_with_passing_verify() {
    let (_dir, beans_dir) = setup_mcp_env();
    let result = tools::handle_tool_call("close_bean", &json!({"id": "1"}), &beans_dir);

    assert!(result.get("isError").is_none());
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Closed bean 1"));
}

#[test]
fn mcp_close_bean_with_failing_verify_returns_error() {
    let (_dir, beans_dir) = setup_mcp_env();

    // Create a bean with a failing verify
    let mut bean = Bean::new("10", "Failing bean");
    bean.slug = Some("failing-bean".to_string());
    bean.verify = Some("false".to_string());
    bean.to_file(beans_dir.join("10-failing-bean.md")).unwrap();
    let index = Index::build(&beans_dir).unwrap();
    index.save(&beans_dir).unwrap();

    let result = tools::handle_tool_call("close_bean", &json!({"id": "10"}), &beans_dir);

    assert_eq!(result["isError"], true);
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Verify failed"));
}

#[test]
fn mcp_close_bean_force_skips_verify() {
    let (_dir, beans_dir) = setup_mcp_env();

    // Create a bean with a failing verify
    let mut bean = Bean::new("10", "Failing bean");
    bean.slug = Some("failing-bean".to_string());
    bean.verify = Some("false".to_string());
    bean.to_file(beans_dir.join("10-failing-bean.md")).unwrap();
    let index = Index::build(&beans_dir).unwrap();
    index.save(&beans_dir).unwrap();

    let result = tools::handle_tool_call(
        "close_bean",
        &json!({"id": "10", "force": true}),
        &beans_dir,
    );

    assert!(result.get("isError").is_none());
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Closed bean 10"));
}

// ---------------------------------------------------------------------------
// Tool handlers: create + close roundtrip
// ---------------------------------------------------------------------------

#[test]
fn mcp_create_then_close_roundtrip() {
    let (_dir, beans_dir) = setup_mcp_env();

    // Create
    let create_result = tools::handle_tool_call(
        "create_bean",
        &json!({
            "title": "Roundtrip test",
            "verify": "echo ok"
        }),
        &beans_dir,
    );
    assert!(create_result.get("isError").is_none());
    let text = create_result["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Created bean 4"));

    // Close
    let close_result = tools::handle_tool_call("close_bean", &json!({"id": "4"}), &beans_dir);
    assert!(close_result.get("isError").is_none());
    let text = close_result["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Closed bean 4"));

    // Verify it's closed in the index
    let index = Index::load_or_rebuild(&beans_dir).unwrap();
    let entry = index.beans.iter().find(|e| e.id == "4");
    // Closed beans get archived, so they may or may not appear in index
    // depending on archive scanning. The key thing is it didn't error.
    if let Some(e) = entry {
        assert_eq!(format!("{}", e.status), "closed");
    }
}

// ---------------------------------------------------------------------------
// Tool handlers: status
// ---------------------------------------------------------------------------

#[test]
fn mcp_status_overview() {
    let (_dir, beans_dir) = setup_mcp_env();
    let result = tools::handle_tool_call("status", &json!({}), &beans_dir);

    assert!(result.get("isError").is_none());
    let text = result["content"][0]["text"].as_str().unwrap();
    let parsed: Value = serde_json::from_str(text).unwrap();

    // 0 claimed, 1 ready (bean 1), 1 goal (bean 3, no verify), 1 blocked (bean 2)
    assert_eq!(parsed["claimed"].as_array().unwrap().len(), 0);
    assert_eq!(parsed["ready"].as_array().unwrap().len(), 1);
    assert_eq!(parsed["goals"].as_array().unwrap().len(), 1);
    assert_eq!(parsed["blocked"].as_array().unwrap().len(), 1);
}

// ---------------------------------------------------------------------------
// Tool handlers: tree
// ---------------------------------------------------------------------------

#[test]
fn mcp_tree_shows_all_beans() {
    let (_dir, beans_dir) = setup_mcp_env();
    let result = tools::handle_tool_call("tree", &json!({}), &beans_dir);

    assert!(result.get("isError").is_none());
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Fix login bug"));
    assert!(text.contains("Add tests for login"));
    assert!(text.contains("Refactor auth module"));
}

#[test]
fn mcp_tree_with_parent_child() {
    let (_dir, beans_dir) = setup_mcp_env();

    // Create a child bean
    let mut child = Bean::new("1.1", "Login unit tests");
    child.slug = Some("login-unit-tests".to_string());
    child.parent = Some("1".to_string());
    child
        .to_file(beans_dir.join("1.1-login-unit-tests.md"))
        .unwrap();
    let index = Index::build(&beans_dir).unwrap();
    index.save(&beans_dir).unwrap();

    let result = tools::handle_tool_call("tree", &json!({"id": "1"}), &beans_dir);
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Fix login bug"));
    assert!(text.contains("Login unit tests"));
}

// ---------------------------------------------------------------------------
// Tool handlers: context_bean
// ---------------------------------------------------------------------------

#[test]
fn mcp_context_bean_no_paths() {
    let (_dir, beans_dir) = setup_mcp_env();
    let result = tools::handle_tool_call("context_bean", &json!({"id": "1"}), &beans_dir);

    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("no file paths"));
}

// ---------------------------------------------------------------------------
// Tool handlers: unknown tool
// ---------------------------------------------------------------------------

#[test]
fn mcp_unknown_tool_returns_error() {
    let (_dir, beans_dir) = setup_mcp_env();
    let result = tools::handle_tool_call("nonexistent_tool", &json!({}), &beans_dir);

    assert_eq!(result["isError"], true);
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Unknown tool"));
}

// ---------------------------------------------------------------------------
// Resource definitions
// ---------------------------------------------------------------------------

#[test]
fn mcp_resource_definitions_present() {
    let defs = resources::resource_definitions();
    assert!(defs.len() >= 2);

    let uris: Vec<&str> = defs.iter().map(|r| r.uri.as_str()).collect();
    assert!(uris.contains(&"beans://status"));
    assert!(uris.contains(&"beans://rules"));
}

// ---------------------------------------------------------------------------
// Resource handlers
// ---------------------------------------------------------------------------

#[test]
fn mcp_resource_read_status() {
    let (_dir, beans_dir) = setup_mcp_env();
    let contents = resources::handle_resource_read("beans://status", &beans_dir).unwrap();

    assert_eq!(contents.len(), 1);
    assert_eq!(contents[0].uri, "beans://status");
    let parsed: Value = serde_json::from_str(&contents[0].text).unwrap();
    assert!(parsed["total"].as_u64().unwrap() >= 3);
}

#[test]
fn mcp_resource_read_rules_missing() {
    let (_dir, beans_dir) = setup_mcp_env();
    let contents = resources::handle_resource_read("beans://rules", &beans_dir).unwrap();

    assert_eq!(contents.len(), 1);
    assert!(contents[0].text.contains("No RULES.md"));
}

#[test]
fn mcp_resource_read_rules_present() {
    let (dir, beans_dir) = setup_mcp_env();
    // Create a RULES.md in project root
    fs::write(dir.path().join("RULES.md"), "# Project Rules\nUse Rust.").unwrap();

    let contents = resources::handle_resource_read("beans://rules", &beans_dir).unwrap();

    assert_eq!(contents.len(), 1);
    assert!(contents[0].text.contains("Use Rust"));
}

#[test]
fn mcp_resource_read_bean() {
    let (_dir, beans_dir) = setup_mcp_env();
    let contents = resources::handle_resource_read("beans://bean/1", &beans_dir).unwrap();

    assert_eq!(contents.len(), 1);
    assert_eq!(contents[0].uri, "beans://bean/1");
    let parsed: Value = serde_json::from_str(&contents[0].text).unwrap();
    assert_eq!(parsed["title"], "Fix login bug");
}

#[test]
fn mcp_resource_read_unknown_uri_returns_error() {
    let (_dir, beans_dir) = setup_mcp_env();
    let result = resources::handle_resource_read("beans://nonexistent", &beans_dir);
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Server dispatch (unit-level, no actual stdio)
// ---------------------------------------------------------------------------

#[test]
fn mcp_server_dispatch_initialize() {
    // We can test the dispatch indirectly through the tool definitions
    // The initialize handler is private, but we verify the protocol response
    // format by testing tool_definitions and resource_definitions produce
    // valid JSON that matches the MCP spec structure.

    let tools = tools::tool_definitions();
    for tool in &tools {
        let json = json!({
            "name": tool.name,
            "description": tool.description,
            "inputSchema": tool.input_schema,
        });
        // Must be valid JSON and have required MCP fields
        assert!(json["name"].is_string());
        assert!(json["description"].is_string());
        assert!(json["inputSchema"].is_object());
    }
}

#[test]
fn mcp_tool_call_result_format_matches_spec() {
    // MCP spec requires tool results to have content array with type+text
    let (_dir, beans_dir) = setup_mcp_env();
    let result = tools::handle_tool_call("status", &json!({}), &beans_dir);

    assert!(result["content"].is_array());
    let content = &result["content"][0];
    assert_eq!(content["type"], "text");
    assert!(content["text"].is_string());
}

#[test]
fn mcp_error_result_has_is_error_flag() {
    let (_dir, beans_dir) = setup_mcp_env();
    let result = tools::handle_tool_call("show_bean", &json!({"id": "999"}), &beans_dir);

    assert_eq!(result["isError"], true);
    assert!(result["content"].is_array());
    assert_eq!(result["content"][0]["type"], "text");
}
