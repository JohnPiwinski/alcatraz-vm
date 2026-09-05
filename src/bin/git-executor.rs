use alcatraz_guest::{
    authorize_channel, decode, digest, encode, validate_git_request, Frame, GitRequest, Operation,
};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const MAX_OUTPUT: usize = 8 * 1024;

fn response(request: &Frame, operation: Operation, payload: serde_json::Value) -> Frame {
    let bytes = serde_json::to_vec(&payload).expect("response payload is serializable");
    Frame {
        version: 1,
        channel: "git-to-codex".into(),
        request_id: request.request_id.clone(),
        operation,
        nonce: format!("response-{}", request.nonce),
        payload_sha256: digest(&bytes),
        payload,
    }
}

fn repo(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let canonical_root = root.canonicalize().map_err(|_| "Git root unavailable")?;
    let canonical = root
        .join(relative)
        .canonicalize()
        .map_err(|_| "repository unavailable")?;
    (canonical.starts_with(&canonical_root) && canonical.join(".git").exists())
        .then_some(canonical)
        .ok_or_else(|| "repository is outside Git root or is not a repository".into())
}

fn execute(root: &Path, request: &Frame) -> Result<serde_json::Value, String> {
    let typed: GitRequest =
        serde_json::from_value(request.payload.clone()).map_err(|_| "invalid Git payload")?;
    let prefix = std::env::var("ALCATRAZ_ALLOWED_REMOTE_PREFIX")
        .unwrap_or_else(|_| "ssh://git.example/".into());
    validate_git_request(&typed, &prefix)?;
    let path = repo(root, &typed.repository)?;
    let (args, label): (Vec<String>, &str) = match request.operation {
        Operation::Status => (
            vec![
                "-C".into(),
                path.display().to_string(),
                "status".into(),
                "--porcelain=v1".into(),
            ],
            "status",
        ),
        Operation::Fetch => (
            vec![
                "-C".into(),
                path.display().to_string(),
                "fetch".into(),
                "--no-tags".into(),
                typed.remote.clone(),
                typed.branch.clone(),
            ],
            "fetch",
        ),
        Operation::ScopedPush => (
            vec![
                "-C".into(),
                path.display().to_string(),
                "push".into(),
                typed.remote.clone(),
                format!("{}:{}", typed.branch, typed.branch),
            ],
            "scoped_push",
        ),
        _ => return Err("unsupported Git operation".into()),
    };
    let output = Command::new("git")
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .output()
        .map_err(|_| "fixed Git executor failed to start")?;
    let mut bytes = output.stdout;
    bytes.extend_from_slice(&output.stderr);
    let truncated = bytes.len() > MAX_OUTPUT;
    bytes.truncate(MAX_OUTPUT);
    Ok(
        serde_json::json!({"operation":label,"success":output.status.success(),"output":String::from_utf8_lossy(&bytes),"output_truncated":truncated}),
    )
}

fn main() {
    let root = PathBuf::from(std::env::var_os("ALCATRAZ_GIT_ROOT").unwrap_or_else(|| ".".into()));
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
            execute(&root, &request).map(|p| response(&request, Operation::Result, p))
        });
        let frame = match result {
            Ok(v) => v,
            Err(error) => match serde_json::from_str::<Frame>(&line).ok() {
                Some(request) => response(
                    &request,
                    Operation::Failure,
                    serde_json::json!({"error":error}),
                ),
                None => continue,
            },
        };
        if let Ok(bytes) = encode(&frame) {
            let _ = stdout.write_all(&bytes);
            let _ = stdout.flush();
        }
    }
}
