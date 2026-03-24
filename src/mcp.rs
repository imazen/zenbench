//! MCP (Model Context Protocol) server for benchmark management.
//!
//! Implements a JSON-RPC 2.0 stdio server that exposes benchmark
//! management tools: list runs, check status, kill, spawn, get results.

use crate::daemon;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

/// Run the MCP server, reading JSON-RPC from stdin and writing to stdout.
pub fn run_server(project_root: PathBuf) {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };

        if line.trim().is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let error_response =
                    json_rpc_error(Value::Null, -32700, &format!("Parse error: {e}"));
                let _ = writeln!(
                    stdout,
                    "{}",
                    serde_json::to_string(&error_response).unwrap()
                );
                let _ = stdout.flush();
                continue;
            }
        };

        let response = handle_request(&request, &project_root);
        let _ = writeln!(stdout, "{}", serde_json::to_string(&response).unwrap());
        let _ = stdout.flush();
    }
}

fn handle_request(request: &JsonRpcRequest, project_root: &Path) -> JsonRpcResponse {
    match request.method.as_str() {
        "initialize" => handle_initialize(request),
        "notifications/initialized" | "initialized" => {
            // Notification — no response required by spec, but we respond for robustness
            JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id.clone(),
                result: Some(Value::Null),
                error: None,
            }
        }
        "tools/list" => handle_tools_list(request),
        "tools/call" => handle_tools_call(request, project_root),
        "ping" => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id.clone(),
            result: Some(Value::Object(serde_json::Map::new())),
            error: None,
        },
        _ => json_rpc_error(
            request.id.clone(),
            -32601,
            &format!("Method not found: {}", request.method),
        ),
    }
}

fn handle_initialize(request: &JsonRpcRequest) -> JsonRpcResponse {
    let result = serde_json::json!({
        "protocolVersion": "2025-03-26",
        "capabilities": {
            "tools": {}
        },
        "serverInfo": {
            "name": "zenbench",
            "version": env!("CARGO_PKG_VERSION")
        }
    });

    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: request.id.clone(),
        result: Some(result),
        error: None,
    }
}

fn handle_tools_list(request: &JsonRpcRequest) -> JsonRpcResponse {
    let tools = serde_json::json!({
        "tools": [
            {
                "name": "list_runs",
                "description": "List all benchmark runs (active and completed). Status is auto-reconciled: dead processes are detected as completed or failed.",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "required": []
                }
            },
            {
                "name": "run_status",
                "description": "Get detailed status of a specific benchmark run, including whether the process is alive and if results exist.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "run_id": {
                            "type": "string",
                            "description": "The run ID to check"
                        }
                    },
                    "required": ["run_id"]
                }
            },
            {
                "name": "kill_run",
                "description": "Kill a running benchmark by ID, or 'stale' to kill all stale runs from previous git commits.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "target": {
                            "type": "string",
                            "description": "Run ID to kill, or 'stale' to kill runs from old git commits"
                        }
                    },
                    "required": ["target"]
                }
            },
            {
                "name": "spawn_bench",
                "description": "Spawn a benchmark as a detached background process. The process runs independently and results are saved to a JSON file. Use get_results or wait_for_results to retrieve them.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "bench_name": {
                            "type": "string",
                            "description": "Benchmark target name (as in `cargo bench --bench <name>`)"
                        },
                        "command": {
                            "type": "string",
                            "description": "Command to run (default: 'cargo')"
                        },
                        "args": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Full argument list. If bench_name is provided, this is ignored and args are built automatically."
                        }
                    },
                    "required": []
                }
            },
            {
                "name": "get_results",
                "description": "Get benchmark results for a completed run. Use run_id='latest' for most recent completed run with results. Can also pass a path to a results JSON file.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "run_id": {
                            "type": "string",
                            "description": "Run ID, 'latest' for most recent, or path to a results JSON file"
                        },
                        "format": {
                            "type": "string",
                            "enum": ["json", "markdown", "csv"],
                            "description": "Output format (default: json)"
                        }
                    },
                    "required": ["run_id"]
                }
            },
            {
                "name": "wait_for_results",
                "description": "Wait for a benchmark run to complete and return its results. Polls every 2 seconds until complete or timeout.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "run_id": {
                            "type": "string",
                            "description": "Run ID to wait for"
                        },
                        "timeout_secs": {
                            "type": "integer",
                            "description": "Timeout in seconds (default: 600)"
                        },
                        "format": {
                            "type": "string",
                            "enum": ["json", "markdown", "csv"],
                            "description": "Output format (default: markdown)"
                        }
                    },
                    "required": ["run_id"]
                }
            },
            {
                "name": "compare_results",
                "description": "Compare two benchmark result files and return the comparison with percentage changes.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "baseline_path": {
                            "type": "string",
                            "description": "Path to baseline results JSON file"
                        },
                        "candidate_path": {
                            "type": "string",
                            "description": "Path to candidate results JSON file"
                        }
                    },
                    "required": ["baseline_path", "candidate_path"]
                }
            },
            {
                "name": "clean_runs",
                "description": "Clean up old run metadata, stderr logs, and result files.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "max_age_hours": {
                            "type": "integer",
                            "description": "Maximum age in hours (default: 168 = 7 days)"
                        }
                    },
                    "required": []
                }
            }
        ]
    });

    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: request.id.clone(),
        result: Some(tools),
        error: None,
    }
}

fn handle_tools_call(request: &JsonRpcRequest, project_root: &Path) -> JsonRpcResponse {
    let params = request.params.as_ref();

    let tool_name = params
        .and_then(|p| p.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let arguments = params
        .and_then(|p| p.get("arguments"))
        .cloned()
        .unwrap_or(Value::Object(serde_json::Map::new()));

    let result = match tool_name {
        "list_runs" => tool_list_runs(project_root),
        "run_status" => tool_run_status(project_root, &arguments),
        "kill_run" => tool_kill_run(project_root, &arguments),
        "spawn_bench" => tool_spawn_bench(project_root, &arguments),
        "get_results" => tool_get_results(project_root, &arguments),
        "wait_for_results" => tool_wait_for_results(project_root, &arguments),
        "compare_results" => tool_compare_results(&arguments),
        "clean_runs" => tool_clean_runs(project_root, &arguments),
        _ => Err(format!("Unknown tool: {tool_name}")),
    };

    match result {
        Ok(content) => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id.clone(),
            result: Some(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": content
                }]
            })),
            error: None,
        },
        Err(e) => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id.clone(),
            result: Some(serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": e
                }],
                "isError": true
            })),
            error: None,
        },
    }
}

// --- Tool implementations ---

fn tool_list_runs(project_root: &Path) -> Result<String, String> {
    let runs = daemon::list_runs(project_root).map_err(|e| format!("Error: {e}"))?;

    if runs.is_empty() {
        return Ok("No benchmark runs found.".to_string());
    }

    let output: Vec<Value> = runs
        .iter()
        .map(|run| {
            serde_json::json!({
                "id": run.id,
                "status": format_status(&run.status),
                "pid": run.pid,
                "git_hash": run.git_hash,
                "command": run.command,
                "started_at": run.started_at,
                "finished_at": run.finished_at,
                "alive": daemon::is_process_alive(run.pid),
                "has_results": run.result_path.as_ref().is_some_and(|p| p.exists()),
            })
        })
        .collect();

    serde_json::to_string_pretty(&output).map_err(|e| format!("Serialization error: {e}"))
}

fn tool_run_status(project_root: &Path, args: &Value) -> Result<String, String> {
    let run_id = args
        .get("run_id")
        .and_then(|v| v.as_str())
        .ok_or("Missing required argument: run_id")?;

    let state = daemon::load_run_state(project_root, run_id)
        .map_err(|e| format!("Error loading run {run_id}: {e}"))?;

    let alive = daemon::is_process_alive(state.pid);
    let has_results = state.result_path.as_ref().is_some_and(|p| p.exists());
    let output = serde_json::json!({
        "id": state.id,
        "status": format_status(&state.status),
        "pid": state.pid,
        "alive": alive,
        "git_hash": state.git_hash,
        "command": state.command,
        "started_at": state.started_at,
        "finished_at": state.finished_at,
        "result_path": state.result_path,
        "has_results": has_results,
    });

    serde_json::to_string_pretty(&output).map_err(|e| format!("Serialization error: {e}"))
}

fn tool_kill_run(project_root: &Path, args: &Value) -> Result<String, String> {
    let target = args
        .get("target")
        .and_then(|v| v.as_str())
        .ok_or("Missing required argument: target")?;

    if target == "stale" {
        let hash = crate::platform::git_commit_hash().unwrap_or_default();
        if hash.is_empty() {
            return Err("Cannot determine current git hash.".to_string());
        }
        let killed =
            daemon::kill_stale_runs(project_root, &hash).map_err(|e| format!("Error: {e}"))?;
        Ok(format!("Killed {killed} stale run(s)."))
    } else {
        let killed = daemon::kill_run(project_root, target).map_err(|e| format!("Error: {e}"))?;
        if killed {
            Ok(format!("Killed run {target}."))
        } else {
            Ok(format!("Run {target} was not running."))
        }
    }
}

fn tool_spawn_bench(project_root: &Path, args: &Value) -> Result<String, String> {
    // Support both bench_name (simple) and command+args (advanced)
    if let Some(bench_name) = args.get("bench_name").and_then(|v| v.as_str()) {
        let spawn_args = vec!["bench", "--bench", bench_name];
        let run_id = daemon::spawn_fire_and_forget(project_root, "cargo", &spawn_args)
            .map_err(|e| format!("Error spawning: {e}"))?;
        return Ok(serde_json::to_string_pretty(&serde_json::json!({
            "run_id": run_id,
            "message": format!("Spawned benchmark '{bench_name}' as run {run_id}. Use wait_for_results or get_results to retrieve results."),
        }))
        .unwrap());
    }

    let command = args
        .get("command")
        .and_then(|v| v.as_str())
        .ok_or("Missing required argument: bench_name or command")?;

    let args_array = args
        .get("args")
        .and_then(|v| v.as_array())
        .ok_or("Missing required argument: args (when using command)")?;

    let str_args: Vec<&str> = args_array.iter().filter_map(|v| v.as_str()).collect();

    let run_id = daemon::spawn_fire_and_forget(project_root, command, &str_args)
        .map_err(|e| format!("Error spawning: {e}"))?;

    Ok(serde_json::to_string_pretty(&serde_json::json!({
        "run_id": run_id,
        "message": format!("Spawned run {run_id}. Use wait_for_results or get_results to retrieve results."),
    }))
    .unwrap())
}

fn tool_get_results(project_root: &Path, args: &Value) -> Result<String, String> {
    let run_id = args
        .get("run_id")
        .and_then(|v| v.as_str())
        .ok_or("Missing required argument: run_id")?;

    let format = args
        .get("format")
        .and_then(|v| v.as_str())
        .unwrap_or("json");

    // Check if it's a direct file path
    let file_path = std::path::PathBuf::from(run_id);
    if file_path.exists() && file_path.extension().is_some_and(|e| e == "json") {
        let result = crate::SuiteResult::load(&file_path)
            .map_err(|e| format!("Error loading results: {e}"))?;
        return format_result(&result, format);
    }

    let actual_id = if run_id == "latest" {
        let state = daemon::find_latest_with_results(project_root)
            .map_err(|e| format!("Error: {e}"))?
            .ok_or("No completed runs with results found.")?;
        state.id
    } else {
        run_id.to_string()
    };

    let state =
        daemon::load_run_state(project_root, &actual_id).map_err(|e| format!("Error: {e}"))?;

    let result_path = state.result_path.as_ref().ok_or(format!(
        "Run {} has no results yet (status: {})",
        actual_id,
        format_status(&state.status)
    ))?;

    let result =
        crate::SuiteResult::load(result_path).map_err(|e| format!("Error loading results: {e}"))?;

    format_result(&result, format)
}

fn tool_wait_for_results(project_root: &Path, args: &Value) -> Result<String, String> {
    let run_id = args
        .get("run_id")
        .and_then(|v| v.as_str())
        .ok_or("Missing required argument: run_id")?;

    let timeout_secs = args
        .get("timeout_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(600);

    let format = args
        .get("format")
        .and_then(|v| v.as_str())
        .unwrap_or("markdown");

    let state = daemon::wait_for_run(
        project_root,
        run_id,
        std::time::Duration::from_secs(2),
        std::time::Duration::from_secs(timeout_secs),
    )
    .map_err(|e| format!("Error: {e}"))?;

    match &state.status {
        daemon::RunStatus::Completed => {
            let result_path = state.result_path.as_ref().ok_or("No result path")?;
            let result = crate::SuiteResult::load(result_path)
                .map_err(|e| format!("Error loading results: {e}"))?;
            format_result(&result, format)
        }
        daemon::RunStatus::Failed(msg) => Err(format!("Benchmark failed: {msg}")),
        daemon::RunStatus::Killed => Err("Benchmark was killed.".to_string()),
        other => Err(format!("Unexpected status: {}", format_status(other))),
    }
}

fn tool_compare_results(args: &Value) -> Result<String, String> {
    let baseline_path = args
        .get("baseline_path")
        .and_then(|v| v.as_str())
        .ok_or("Missing required argument: baseline_path")?;

    let candidate_path = args
        .get("candidate_path")
        .and_then(|v| v.as_str())
        .ok_or("Missing required argument: candidate_path")?;

    let baseline = crate::SuiteResult::load(baseline_path)
        .map_err(|e| format!("Error loading baseline: {e}"))?;
    let candidate = crate::SuiteResult::load(candidate_path)
        .map_err(|e| format!("Error loading candidate: {e}"))?;

    let mut comparisons = Vec::new();

    // Compare groups
    for cand_group in &candidate.comparisons {
        if let Some(base_group) = baseline
            .comparisons
            .iter()
            .find(|g| g.group_name == cand_group.group_name)
        {
            for cand_bench in &cand_group.benchmarks {
                if let Some(base_bench) = base_group
                    .benchmarks
                    .iter()
                    .find(|b| b.name == cand_bench.name)
                {
                    let pct = if base_bench.summary.mean.abs() > f64::EPSILON {
                        (cand_bench.summary.mean - base_bench.summary.mean)
                            / base_bench.summary.mean
                            * 100.0
                    } else {
                        0.0
                    };
                    comparisons.push(serde_json::json!({
                        "group": cand_group.group_name,
                        "benchmark": cand_bench.name,
                        "baseline_mean_ns": base_bench.summary.mean,
                        "candidate_mean_ns": cand_bench.summary.mean,
                        "pct_change": pct,
                        "baseline_formatted": crate::format::format_ns(base_bench.summary.mean),
                        "candidate_formatted": crate::format::format_ns(cand_bench.summary.mean),
                    }));
                }
            }
        }
    }

    // Compare standalones
    for cand_bench in &candidate.standalones {
        if let Some(base_bench) = baseline
            .standalones
            .iter()
            .find(|b| b.name == cand_bench.name)
        {
            let pct = if base_bench.summary.mean.abs() > f64::EPSILON {
                (cand_bench.summary.mean - base_bench.summary.mean) / base_bench.summary.mean
                    * 100.0
            } else {
                0.0
            };
            comparisons.push(serde_json::json!({
                "benchmark": cand_bench.name,
                "baseline_mean_ns": base_bench.summary.mean,
                "candidate_mean_ns": cand_bench.summary.mean,
                "pct_change": pct,
                "baseline_formatted": crate::format::format_ns(base_bench.summary.mean),
                "candidate_formatted": crate::format::format_ns(cand_bench.summary.mean),
            }));
        }
    }

    serde_json::to_string_pretty(&comparisons).map_err(|e| format!("Serialization error: {e}"))
}

fn tool_clean_runs(project_root: &Path, args: &Value) -> Result<String, String> {
    let max_age_hours = args
        .get("max_age_hours")
        .and_then(|v| v.as_u64())
        .unwrap_or(168);

    let cleaned = daemon::cleanup_old_runs(project_root, max_age_hours * 3600)
        .map_err(|e| format!("Error: {e}"))?;

    Ok(format!("Cleaned up {cleaned} old run(s)."))
}

// --- Helpers ---

fn format_status(status: &daemon::RunStatus) -> String {
    match status {
        daemon::RunStatus::Queued => "queued".to_string(),
        daemon::RunStatus::Running => "running".to_string(),
        daemon::RunStatus::Completed => "completed".to_string(),
        daemon::RunStatus::Failed(msg) => format!("failed: {msg}"),
        daemon::RunStatus::Killed => "killed".to_string(),
    }
}

fn format_result(result: &crate::SuiteResult, format: &str) -> Result<String, String> {
    match format {
        "markdown" => Ok(result.to_markdown()),
        "csv" => Ok(result.to_csv()),
        _ => serde_json::to_string_pretty(result).map_err(|e| format!("Serialization error: {e}")),
    }
}

// --- JSON-RPC types ---

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Value,
    method: String,
    params: Option<Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<Value>,
}

fn json_rpc_error(id: Value, code: i32, message: &str) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id,
        result: None,
        error: Some(serde_json::json!({
            "code": code,
            "message": message
        })),
    }
}
