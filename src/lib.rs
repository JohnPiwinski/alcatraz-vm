//! Bounded guest-side protocol and authorization primitives.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

pub const MAX_RECORD: usize = 16 * 1024;
pub const MAX_PAYLOAD: usize = 8 * 1024;

/// Return a greeting for the supplied name.
pub fn greet(name: &str) -> String {
    format!("Hello, {name}!")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    VibeKanban,
    Codex,
    Git,
    Build,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    SubmitTask,
    StatusQuery,
    OperatorReview,
    Status,
    Result,
    Failure,
    Fetch,
    ScopedPush,
    SendBundle,
    Check,
    Test,
    Run,
    Reset,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Frame {
    pub version: u8,
    pub channel: String,
    pub request_id: String,
    pub operation: Operation,
    pub nonce: String,
    pub payload_sha256: String,
    pub payload: serde_json::Value,
}

pub fn digest(payload: &[u8]) -> String {
    format!("{:x}", Sha256::digest(payload))
}

pub fn encode(frame: &Frame) -> Result<Vec<u8>, String> {
    let payload = serde_json::to_vec(&frame.payload).map_err(|e| e.to_string())?;
    if payload.len() > MAX_PAYLOAD {
        return Err("payload too large".into());
    }
    if frame.version != 1
        || frame.request_id.is_empty()
        || frame.request_id.len() > 128
        || frame.nonce.is_empty()
        || frame.nonce.len() > 128
        || frame.channel.len() > 64
    {
        return Err("invalid frame bounds".into());
    }
    if frame.payload_sha256 != digest(&payload) {
        return Err("payload digest mismatch".into());
    }
    let mut out = serde_json::to_vec(frame).map_err(|e| e.to_string())?;
    out.push(b'\n');
    if out.len() > MAX_RECORD {
        return Err("record too large".into());
    }
    Ok(out)
}

pub fn decode(line: &[u8]) -> Result<Frame, String> {
    if line.len() > MAX_RECORD {
        return Err("record too large".into());
    }
    if !line.ends_with(b"\n") {
        return Err("missing record delimiter".into());
    }
    let f: Frame = serde_json::from_slice(line).map_err(|_| "malformed json")?;
    let _ = encode(&f)?;
    Ok(f)
}

pub fn allowed(source: Role, destination: Role, op: Operation) -> bool {
    matches!(
        (source, destination, op),
        (
            Role::VibeKanban,
            Role::Codex,
            Operation::SubmitTask | Operation::StatusQuery | Operation::OperatorReview
        ) | (
            Role::Codex,
            Role::VibeKanban,
            Operation::Status | Operation::Result | Operation::Failure
        ) | (
            Role::Codex,
            Role::Git,
            Operation::Status | Operation::Fetch | Operation::ScopedPush
        ) | (
            Role::Git,
            Role::Codex,
            Operation::Status | Operation::Result | Operation::Failure
        ) | (
            Role::Codex,
            Role::Build,
            Operation::SendBundle
                | Operation::Check
                | Operation::Test
                | Operation::Run
                | Operation::Reset
        ) | (
            Role::Build,
            Role::Codex,
            Operation::Result | Operation::Failure
        )
    )
}

/// Enforce endpoint identity before an operation payload is handled.
pub fn authorize_channel(channel: &str, op: Operation) -> Result<(Role, Role), String> {
    let (source, destination) = match channel {
        "vibe-to-codex" => (Role::VibeKanban, Role::Codex),
        "codex-to-vibe" => (Role::Codex, Role::VibeKanban),
        "codex-to-git" => (Role::Codex, Role::Git),
        "git-to-codex" => (Role::Git, Role::Codex),
        "codex-to-build" => (Role::Codex, Role::Build),
        "build-to-codex" => (Role::Build, Role::Codex),
        _ => return Err("unknown channel".into()),
    };
    allowed(source, destination, op)
        .then_some((source, destination))
        .ok_or_else(|| "operation denied on channel".into())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitRequest {
    pub repository: String,
    pub remote: String,
    pub branch: String,
    #[serde(default)]
    pub force: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildRequest {
    pub workspace: String,
    pub operation: Operation,
    pub bundle_sha256: String,
}

fn safe_component(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.bytes().any(|b| b < 0x20 || b == 0x7f)
}

/// Validate Git policy before invoking a Git helper. The helper receives only
/// this validated structure; it never receives a shell command.
pub fn validate_git_request(
    request: &GitRequest,
    allowed_remote_prefix: &str,
) -> Result<(), String> {
    if !safe_component(&request.repository, 256)
        || request.repository.starts_with('/')
        || request
            .repository
            .split('/')
            .any(|part| part == ".." || part.is_empty())
    {
        return Err("invalid repository path".into());
    }
    if !safe_component(&request.branch, 128) || request.branch.starts_with('-') {
        return Err("invalid branch".into());
    }
    if request.force {
        return Err("force push denied".into());
    }
    if !safe_component(&request.remote, 1024) || !request.remote.starts_with(allowed_remote_prefix)
    {
        return Err("remote denied".into());
    }
    Ok(())
}

/// Validate Build requests without interpreting user text as a command.
pub fn validate_build_request(request: &BuildRequest) -> Result<(), String> {
    if !matches!(
        request.operation,
        Operation::Check | Operation::Test | Operation::Run | Operation::Reset
    ) {
        return Err("operation is not a Build operation".into());
    }
    if !safe_component(&request.workspace, 256)
        || request.workspace.starts_with('/')
        || request
            .workspace
            .split('/')
            .any(|part| part == ".." || part.is_empty())
    {
        return Err("invalid workspace".into());
    }
    if request.bundle_sha256.len() != 64
        || !request.bundle_sha256.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("invalid bundle digest".into());
    }
    Ok(())
}

#[derive(Default)]
pub struct ReplayGuard {
    seen: HashSet<String>,
}
impl ReplayGuard {
    pub fn accept(&mut self, nonce: &str) -> bool {
        self.seen.insert(nonce.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn greeting_is_exact() {
        assert_eq!(greet("Alcatraz"), "Hello, Alcatraz!");
    }

    #[test]
    fn greeting_handles_empty_names() {
        assert_eq!(greet(""), "Hello, !");
    }

    #[test]
    fn greeting_preserves_name_content() {
        assert_eq!(greet("Ada Lovelace"), "Hello, Ada Lovelace!");
        assert_eq!(greet("R&D!"), "Hello, R&D!!");
    }

    fn frame(op: Operation) -> Frame {
        let p = json!({"x":"y"});
        Frame {
            version: 1,
            channel: "codex-to-build".into(),
            request_id: "r".into(),
            operation: op,
            nonce: "n".into(),
            payload_sha256: digest(&serde_json::to_vec(&p).unwrap()),
            payload: p,
        }
    }
    #[test]
    fn round_trip() {
        let f = frame(Operation::Check);
        assert_eq!(decode(&encode(&f).unwrap()).unwrap().request_id, "r");
    }
    #[test]
    fn malformed_and_unframed_records_fail_closed() {
        assert!(decode(b"not-json\n").is_err());
        let f = frame(Operation::Check);
        let mut bytes = encode(&f).unwrap();
        bytes.pop();
        assert!(decode(&bytes).is_err());
    }
    #[test]
    fn unknown_fields_fail_closed() {
        let mut bytes = encode(&frame(Operation::Check)).unwrap();
        bytes.insert(bytes.len() - 1, b'}');
        assert!(decode(&bytes).is_err());
    }
    #[test]
    fn oversized_records_fail_closed() {
        assert!(decode(&vec![b'x'; MAX_RECORD + 1]).is_err());
    }
    #[test]
    fn denies_spoke_to_spoke() {
        assert!(!allowed(
            Role::VibeKanban,
            Role::Build,
            Operation::SubmitTask
        ));
    }
    #[test]
    fn replay_is_rejected() {
        let mut g = ReplayGuard::default();
        assert!(g.accept("n"));
        assert!(!g.accept("n"));
    }
    #[test]
    fn digest_and_size_are_checked() {
        let mut f = frame(Operation::Check);
        f.payload_sha256 = "bad".into();
        assert!(encode(&f).is_err());
    }
    #[test]
    fn channel_identity_authorizes_before_payload() {
        assert!(authorize_channel("vibe-to-codex", Operation::SubmitTask).is_ok());
        assert!(authorize_channel("vibe-to-codex", Operation::Run).is_err());
        assert!(authorize_channel("vibe-to-git", Operation::Status).is_err());
    }
    #[test]
    fn spoke_to_spoke_matrix_is_empty() {
        for source in [Role::VibeKanban, Role::Git, Role::Build] {
            for destination in [Role::VibeKanban, Role::Git, Role::Build] {
                if source != destination {
                    assert!(!allowed(source, destination, Operation::Status));
                    assert!(!allowed(source, destination, Operation::Result));
                }
            }
        }
    }
    #[test]
    fn git_policy_rejects_force_path_traversal_and_remote_escape() {
        let good = GitRequest {
            repository: "repos/app".into(),
            remote: "ssh://git.example/".into(),
            branch: "main".into(),
            force: false,
        };
        assert!(validate_git_request(&good, "ssh://git.example/").is_ok());
        assert!(validate_git_request(
            &GitRequest {
                repository: "../secret".into(),
                ..good.clone()
            },
            "ssh://git.example/"
        )
        .is_err());
        assert!(validate_git_request(
            &GitRequest {
                force: true,
                ..good.clone()
            },
            "ssh://git.example/"
        )
        .is_err());
        assert!(validate_git_request(
            &GitRequest {
                remote: "ssh://evil.example/".into(),
                ..good
            },
            "ssh://git.example/"
        )
        .is_err());
    }
    #[test]
    fn build_policy_accepts_named_operations_and_rejects_shell_like_inputs() {
        let good = BuildRequest {
            workspace: "workspace/app".into(),
            operation: Operation::Check,
            bundle_sha256: "a".repeat(64),
        };
        assert!(validate_build_request(&good).is_ok());
        assert!(validate_build_request(&BuildRequest {
            workspace: "/host".into(),
            ..good.clone()
        })
        .is_err());
        assert!(validate_build_request(&BuildRequest {
            workspace: "workspace/app".into(),
            operation: Operation::Status,
            ..good
        })
        .is_err());
    }
}
