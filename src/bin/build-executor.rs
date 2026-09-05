use alcatraz_guest::{
    authorize_channel, decode, digest, encode, validate_build_request, BuildRequest, Frame,
    Operation,
};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

const MAX_OUTPUT: usize = 8 * 1024;

fn response(request: &Frame, operation: Operation, payload: serde_json::Value) -> Frame {
    let bytes = serde_json::to_vec(&payload).expect("response payload is serializable");
    Frame {
        version: 1,
        channel: "build-to-codex".into(),
        request_id: request.request_id.clone(),
        operation,
        nonce: format!("response-{}", request.nonce),
        payload_sha256: digest(&bytes),
        payload,
    }
}

fn workspace(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = root.join(relative);
    let canonical_root = root.canonicalize().map_err(|_| "build root unavailable")?;
    let canonical = path.canonicalize().map_err(|_| "workspace unavailable")?;
    canonical
        .starts_with(&canonical_root)
        .then_some(canonical)
        .ok_or_else(|| "workspace escapes build root".into())
}

fn execute(root: &Path, request: &Frame) -> Result<serde_json::Value, String> {
    let typed: BuildRequest =
        serde_json::from_value(request.payload.clone()).map_err(|_| "invalid Build payload")?;
    validate_build_request(&typed)?;
    if typed.operation != request.operation {
        return Err("payload operation does not match channel frame".into());
    }
    if typed.operation == Operation::Reset {
        return Ok(serde_json::json!({"reset": true, "workspace": typed.workspace}));
    }
    let cwd = workspace(root, &typed.workspace)?;
    let subcommand = match typed.operation {
        Operation::Check => "check",
        Operation::Test => "test",
        Operation::Run => "run",
        _ => return Err("unsupported Build operation".into()),
    };
    let output = Command::new("/usr/bin/cargo")
        .args([subcommand, "--offline"])
        .current_dir(cwd)
        .output()
        .map_err(|e| {
            format!(
                "fixed Cargo executor failed to start: {} ({})",
                e.kind(),
                e.raw_os_error()
                    .map_or_else(|| "no-os-error".into(), |code| code.to_string())
            )
        })?;
    let mut bytes = output.stdout;
    bytes.extend_from_slice(&output.stderr);
    bytes.truncate(MAX_OUTPUT);
    let text = String::from_utf8_lossy(&bytes).to_string();
    Ok(
        serde_json::json!({"operation": subcommand, "success": output.status.success(), "output": text, "output_truncated": bytes.len() == MAX_OUTPUT}),
    )
}

fn main() {
    let root = PathBuf::from(std::env::var_os("ALCATRAZ_BUILD_ROOT").unwrap_or_else(|| ".".into()));
    let stdin = io::stdin();
    let mut stdout = io::BufWriter::new(io::stdout().lock());
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(v) => v,
            Err(_) => break,
        };
        let framed = format!("{line}\n");
        let result = decode(framed.as_bytes()).and_then(|request| {
            authorize_channel(&request.channel, request.operation)?;
            execute(&root, &request).map(|payload| response(&request, Operation::Result, payload))
        });
        let frame = match result {
            Ok(frame) => frame,
            Err(error) => {
                let request = serde_json::from_str::<Frame>(&line).ok();
                match request {
                    Some(request) => response(
                        &request,
                        Operation::Failure,
                        serde_json::json!({"error": error}),
                    ),
                    None => continue,
                }
            }
        };
        if let Ok(bytes) = encode(&frame) {
            let _ = stdout.write_all(&bytes);
            let _ = stdout.flush();
        }
    }
}
