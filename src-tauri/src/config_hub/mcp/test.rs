// ── Config Hub: MCP connectivity test ────────────────────────────────────
//
// Answers "does this server actually work?" by performing a real MCP
// `initialize` handshake:
//
//   stdio   — spawn the command, write the request to stdin, read the JSON
//             response from stdout, then kill the process.
//   http/sse — POST the request to the URL and read the response.
//
// A stdio test runs a local command, so it is only ever started by an explicit
// user action, and the child is killed on every exit path (success, error,
// timeout) so nothing is left running.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};

/// How long an http/sse server gets to answer the handshake.
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);

/// How long a stdio server gets. Generous because `npx -y <pkg>` downloads the
/// package on first run, which can take far longer than a network round-trip.
const STDIO_TIMEOUT: Duration = Duration::from_secs(90);

/// The MCP protocol revision we advertise in the handshake.
const PROTOCOL_VERSION: &str = "2024-11-05";

/// Commands that are `.cmd` batch shims on Windows and must run through `cmd /c`.
#[cfg(windows)]
const WINDOWS_WRAP_COMMANDS: &[&str] = &["npx", "npm", "yarn", "pnpm", "node", "bun", "deno"];

/// Build the process for a stdio server.
///
/// On Windows `npx`/`npm`/… are `.cmd` files that `Command::new` cannot resolve
/// directly, so they're launched as `cmd /c <command> …`.
fn build_command(command: &str, args: &[String]) -> Command {
    #[cfg(windows)]
    {
        let name = std::path::Path::new(command)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(command);
        let already_cmd = command.eq_ignore_ascii_case("cmd")
            || command.eq_ignore_ascii_case("cmd.exe")
            || command.eq_ignore_ascii_case("powershell")
            || command.eq_ignore_ascii_case("pwsh");
        if !already_cmd
            && WINDOWS_WRAP_COMMANDS
                .iter()
                .any(|c| name.eq_ignore_ascii_case(c))
        {
            let mut cmd = Command::new("cmd");
            cmd.arg("/c").arg(command).args(args);
            return cmd;
        }
    }
    let mut cmd = Command::new(command);
    cmd.args(args);
    cmd
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpTestResult {
    pub ok: bool,
    pub latency_ms: u64,
    /// The server's reported name/version, when it answered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_version: Option<String>,
    /// Human-readable outcome, shown in the UI either way.
    pub message: String,
}

/// The JSON-RPC `initialize` request every MCP server must answer.
fn initialize_request() -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": { "name": "trajectory-viewer", "version": env!("CARGO_PKG_VERSION") }
        }
    })
}

/// Run the connectivity test for one server spec.
pub fn test_server(spec: &Value) -> McpTestResult {
    let typ = spec.get("type").and_then(|v| v.as_str()).unwrap_or("stdio");
    match typ {
        "stdio" => test_stdio(spec),
        "http" | "sse" => test_http(spec),
        other => McpTestResult {
            ok: false,
            latency_ms: 0,
            server_name: None,
            server_version: None,
            message: format!("Unsupported MCP type: {other}"),
        },
    }
}

/// Spawn the command and complete the handshake over stdin/stdout.
fn test_stdio(spec: &Value) -> McpTestResult {
    let started = Instant::now();
    let command = spec.get("command").and_then(|v| v.as_str()).unwrap_or("");
    if command.trim().is_empty() {
        return fail(started, "stdio server has no command");
    }
    let args: Vec<String> = spec
        .get("args")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let mut cmd = build_command(command, &args);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(env) = spec.get("env").and_then(|v| v.as_object()) {
        for (k, v) in env {
            if let Some(s) = v.as_str() {
                cmd.env(k, s);
            }
        }
    }
    if let Some(cwd) = spec.get("cwd").and_then(|v| v.as_str()) {
        if !cwd.trim().is_empty() {
            cmd.current_dir(cwd);
        }
    }

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) => return fail(started, format!("Cannot start `{command}`: {err}")),
    };

    // Send the initialize request first so the server can start answering while
    // a slow `npx` download finishes.
    let request = format!("{}\n", initialize_request());
    if let Some(stdin) = child.stdin.as_mut() {
        if let Err(err) = stdin
            .write_all(request.as_bytes())
            .and_then(|_| stdin.flush())
        {
            let _ = child.kill();
            return fail(started, format!("Cannot write to `{command}`: {err}"));
        }
    }

    // Read stdout on a worker thread and look for the JSON-RPC response. A
    // server may print banners or log lines first, so we skip anything that
    // isn't a JSON object with a matching `id`.
    let stdout = child.stdout.take();
    let (tx, rx) = std::sync::mpsc::channel();
    if let Some(stdout) = stdout {
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                let Ok(line) = line else { break };
                let trimmed = line.trim();
                if !trimmed.starts_with('{') {
                    continue;
                }
                if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
                    // Ignore notifications/other traffic; the handshake reply
                    // carries our id.
                    if value.get("id").and_then(|v| v.as_i64()) == Some(1) {
                        let _ = tx.send(Ok(trimmed.to_string()));
                        return;
                    }
                }
            }
            let _ = tx.send(Err("server closed its output".to_string()));
        });
    }

    let line = match rx.recv_timeout(STDIO_TIMEOUT) {
        Ok(Ok(line)) => line,
        Ok(Err(err)) => {
            let _ = child.kill();
            return fail(
                started,
                format!("No handshake reply from `{command}`: {err}"),
            );
        }
        Err(_) => {
            let _ = child.kill();
            return fail(
                started,
                format!("No response within {}s", STDIO_TIMEOUT.as_secs()),
            );
        }
    };

    // Always stop the child, whatever the response said.
    let _ = child.kill();
    let _ = child.wait();

    interpret_response(&line, started)
}

/// POST the handshake to an http/sse endpoint.
fn test_http(spec: &Value) -> McpTestResult {
    let started = Instant::now();
    let url = spec.get("url").and_then(|v| v.as_str()).unwrap_or("");
    if url.trim().is_empty() {
        return fail(started, "http/sse server has no url");
    }

    let client = match reqwest::blocking::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .build()
    {
        Ok(client) => client,
        Err(err) => return fail(started, format!("Cannot build HTTP client: {err}")),
    };

    let mut request = client
        .post(url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .json(&initialize_request());
    if let Some(headers) = spec.get("headers").and_then(|v| v.as_object()) {
        for (k, v) in headers {
            if let Some(s) = v.as_str() {
                request = request.header(k, s);
            }
        }
    }

    let response = match request.send() {
        Ok(response) => response,
        Err(err) => return fail(started, format!("Request failed: {err}")),
    };
    let status = response.status();
    let body = response.text().unwrap_or_default();

    if !status.is_success() {
        return fail(started, format!("HTTP {status}"));
    }

    // The response may be plain JSON or an SSE stream. In an SSE stream the
    // JSON sits on a `data:` line, which may follow other fields (e.g. an
    // `event: message` line), so scan every line rather than only the first.
    let payload = extract_json_payload(&body);
    interpret_response(&payload, started)
}

/// Pull the JSON-RPC object out of a response body.
///
/// Handles both a plain JSON body and an SSE stream (`data: {...}` lines,
/// possibly preceded by `event:`/`id:` lines).
fn extract_json_payload(body: &str) -> String {
    for line in body.lines() {
        let trimmed = line.trim();
        let candidate = trimmed
            .strip_prefix("data:")
            .map(str::trim)
            .unwrap_or(trimmed);
        if candidate.starts_with('{') && serde_json::from_str::<Value>(candidate).is_ok() {
            return candidate.to_string();
        }
    }
    body.trim().to_string()
}

/// Read the handshake result out of a JSON-RPC response line.
fn interpret_response(line: &str, started: Instant) -> McpTestResult {
    let latency_ms = started.elapsed().as_millis() as u64;
    let value: Value = match serde_json::from_str(line.trim()) {
        Ok(value) => value,
        Err(_) => {
            // Show a snippet so a format we don't handle is diagnosable.
            let snippet: String = line.trim().chars().take(120).collect();
            return McpTestResult {
                ok: false,
                latency_ms,
                server_name: None,
                server_version: None,
                message: format!("Server responded, but not with valid JSON: {snippet}"),
            };
        }
    };

    if let Some(error) = value.get("error") {
        let message = error
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown error");
        return McpTestResult {
            ok: false,
            latency_ms,
            server_name: None,
            server_version: None,
            message: format!("Server returned an error: {message}"),
        };
    }

    let info = value.get("result").and_then(|r| r.get("serverInfo"));
    McpTestResult {
        ok: true,
        latency_ms,
        server_name: info
            .and_then(|i| i.get("name"))
            .and_then(|v| v.as_str())
            .map(String::from),
        server_version: info
            .and_then(|i| i.get("version"))
            .and_then(|v| v.as_str())
            .map(String::from),
        message: "Handshake succeeded".to_string(),
    }
}

fn fail(started: Instant, message: impl Into<String>) -> McpTestResult {
    McpTestResult {
        ok: false,
        latency_ms: started.elapsed().as_millis() as u64,
        server_name: None,
        server_version: None,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpret_response_reads_server_info() {
        let line = r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","serverInfo":{"name":"test-server","version":"1.2.3"}}}"#;
        let result = interpret_response(line, Instant::now());
        assert!(result.ok);
        assert_eq!(result.server_name.as_deref(), Some("test-server"));
        assert_eq!(result.server_version.as_deref(), Some("1.2.3"));
    }

    #[test]
    fn interpret_response_reports_a_jsonrpc_error() {
        let line =
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"Method not found"}}"#;
        let result = interpret_response(line, Instant::now());
        assert!(!result.ok);
        assert!(
            result.message.contains("Method not found"),
            "got: {}",
            result.message
        );
    }

    #[test]
    fn interpret_response_handles_non_json() {
        let result = interpret_response("not json at all", Instant::now());
        assert!(!result.ok);
        assert!(result.message.contains("not with valid JSON"));
    }

    #[test]
    fn extract_json_payload_passes_through_plain_json() {
        let body = r#"{"jsonrpc":"2.0","id":1,"result":{}}"#;
        assert_eq!(extract_json_payload(body), body);
    }

    // exa and other SSE servers send `event: message` before the `data:` line,
    // so the JSON is not the first thing in the body.
    #[test]
    fn extract_json_payload_reads_data_lines_after_event_lines() {
        let body = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"serverInfo\":{\"name\":\"exa\"}}}\n\n";
        let payload = extract_json_payload(body);
        let value: Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(value["result"]["serverInfo"]["name"], "exa");
    }

    #[test]
    fn extract_json_payload_ignores_non_json_data_lines() {
        let body = "event: ping\ndata: keep-alive\ndata: {\"id\":1}\n";
        let payload = extract_json_payload(body);
        assert_eq!(payload, "{\"id\":1}");
    }

    #[test]
    fn test_server_rejects_an_unsupported_type() {
        let result = test_server(&json!({ "type": "bogus" }));
        assert!(!result.ok);
        assert!(result.message.contains("Unsupported"));
    }

    #[test]
    fn stdio_without_a_command_fails_cleanly() {
        let result = test_server(&json!({ "type": "stdio" }));
        assert!(!result.ok);
        assert!(result.message.contains("no command"));
    }

    #[test]
    fn http_without_a_url_fails_cleanly() {
        let result = test_server(&json!({ "type": "http" }));
        assert!(!result.ok);
        assert!(result.message.contains("no url"));
    }

    // A command that doesn't exist must report a spawn failure, not hang.
    #[test]
    fn stdio_with_a_missing_command_fails_cleanly() {
        let result = test_server(&json!({
            "type": "stdio",
            "command": "definitely-not-a-real-command-xyz",
        }));
        assert!(!result.ok);
        assert!(
            result.message.contains("Cannot start"),
            "got: {}",
            result.message
        );
    }

    // Windows resolves `npx` to npx.cmd, which Command::new can't launch — it
    // has to go through `cmd /c`, or every npx-based server fails to start.
    #[cfg(windows)]
    #[test]
    fn build_command_wraps_npx_in_cmd_on_windows() {
        let cmd = build_command("npx", &["-y".to_string(), "pkg".to_string()]);
        assert_eq!(cmd.get_program().to_string_lossy().to_lowercase(), "cmd");
        let args: Vec<_> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args[0], "/c");
        assert_eq!(args[1], "npx");
        assert_eq!(args[2], "-y");
    }

    #[cfg(windows)]
    #[test]
    fn build_command_does_not_double_wrap_cmd() {
        let cmd = build_command("cmd", &["/c".to_string(), "npx".to_string()]);
        assert_eq!(cmd.get_program().to_string_lossy().to_lowercase(), "cmd");
        let args: Vec<_> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        // Passed straight through, with no extra `/c cmd`.
        assert_eq!(args, vec!["/c", "npx"]);
    }

    // A plain executable (python, uvx, …) must not be wrapped.
    #[cfg(windows)]
    #[test]
    fn build_command_leaves_plain_executables_alone() {
        let cmd = build_command("python", &["server.py".to_string()]);
        assert_eq!(cmd.get_program().to_string_lossy().to_lowercase(), "python");
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;

    /// End-to-end against a real npx-launched server. Needs network on first
    /// run (npx downloads the package). `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn real_npx_server_handshakes() {
        let result = test_server(&json!({
            "type": "stdio",
            "command": "npx",
            "args": ["-y", "@modelcontextprotocol/server-everything"],
        }));
        eprintln!(
            "ok={} latency={}ms name={:?} msg={}",
            result.ok, result.latency_ms, result.server_name, result.message
        );
        assert!(result.ok, "handshake failed: {}", result.message);
    }

    /// End-to-end against a real SSE (http) server, which answers with
    /// `event: message` + `data:` lines. `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn real_sse_server_handshakes() {
        let result = test_server(&json!({
            "type": "http",
            "url": "https://mcp.exa.ai/mcp?tools=web_search_exa",
        }));
        eprintln!(
            "ok={} latency={}ms name={:?} msg={}",
            result.ok, result.latency_ms, result.server_name, result.message
        );
        assert!(result.ok, "handshake failed: {}", result.message);
    }
}
