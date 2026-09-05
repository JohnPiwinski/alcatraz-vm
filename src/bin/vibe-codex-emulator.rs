//! Command-line VibeKanban substitute for exercising the Codex endpoint.
//! It speaks the same bounded Vibe↔Codex protocol and never accepts shell text.
use alcatraz_guest::{authorize_channel, decode, digest, encode, Frame, Operation, MAX_RECORD};
use serde_json::{json, Value};
use std::env;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;

fn usage() -> ! {
    eprintln!(
        "usage: vibe-codex-emulator --socket PATH [--port PORT] \
         submit-task TASK | status TASK_ID | operator-review REASON"
    );
    std::process::exit(2);
}

fn command(args: &[String]) -> Result<(String, u32, Operation, Value), String> {
    let mut i = 0;
    let mut socket = None;
    let mut port = 7000u32;
    while i < args.len() && args[i].starts_with('-') {
        match args[i].as_str() {
            "--socket" => {
                i += 1;
                socket = args.get(i).cloned();
            }
            "--port" => {
                i += 1;
                port = args
                    .get(i)
                    .ok_or("missing port")?
                    .parse()
                    .map_err(|_| "invalid port")?;
                if !(1024..=65535).contains(&port) {
                    return Err("port outside fixed range".into());
                }
            }
            _ => return Err("unknown option".into()),
        }
        i += 1;
    }
    let socket = socket.ok_or("--socket is required")?;
    if i + 1 >= args.len() {
        return Err("missing named command and argument".into());
    }
    if i + 2 != args.len() {
        return Err("named commands accept exactly one bounded argument".into());
    }
    let (op, payload) = match args[i].as_str() {
        "submit-task" => (Operation::SubmitTask, json!({"task": args[i + 1]})),
        "status" => (Operation::StatusQuery, json!({"task_id": args[i + 1]})),
        "operator-review" => (Operation::OperatorReview, json!({"reason": args[i + 1]})),
        _ => return Err("command is not an allowlisted Vibe operation".into()),
    };
    if serde_json::to_vec(&payload)
        .map_err(|_| "payload encoding failed")?
        .len()
        > alcatraz_guest::MAX_PAYLOAD
    {
        return Err("command argument too large".into());
    }
    Ok((socket, port, op, payload))
}

fn bounded_line(stream: &mut UnixStream) -> io::Result<Vec<u8>> {
    let mut out = Vec::with_capacity(256);
    let mut byte = [0u8; 1];
    while out.len() <= MAX_RECORD {
        let read = stream.read(&mut byte)?;
        if read == 0 {
            break;
        }
        out.push(byte[0]);
        if byte[0] == b'\n' {
            return Ok(out);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "bounded record read failed",
    ))
}

fn run(args: &[String]) -> Result<Value, Box<dyn std::error::Error>> {
    let (socket, port, operation, payload) = command(args).map_err(io::Error::other)?;
    authorize_channel("vibe-to-codex", operation).map_err(io::Error::other)?;
    let mut stream = UnixStream::connect(socket)?;
    stream.write_all(format!("CONNECT {port}\n").as_bytes())?;
    let ack = bounded_line(&mut stream)?;
    if !ack.starts_with(b"OK ") {
        return Err(
            io::Error::new(io::ErrorKind::PermissionDenied, "Codex handshake rejected").into(),
        );
    }
    let payload_bytes = serde_json::to_vec(&payload)?;
    let frame = Frame {
        version: 1,
        channel: "vibe-to-codex".into(),
        request_id: format!("vibe-cli-{}", std::process::id()),
        operation,
        nonce: format!("vibe-nonce-{}", std::process::id()),
        payload_sha256: digest(&payload_bytes),
        payload,
    };
    stream.write_all(&encode(&frame).map_err(io::Error::other)?)?;
    let response = decode(&bounded_line(&mut stream)?).map_err(io::Error::other)?;
    authorize_channel(&response.channel, response.operation).map_err(io::Error::other)?;
    if !matches!(response.operation, Operation::Result | Operation::Failure) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Codex returned disallowed operation",
        )
        .into());
    }
    Ok(response.payload)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        usage();
    }
    println!("{}", serde_json::to_string(&run(&args)?)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(command: &str, value: &str) -> Vec<String> {
        vec![
            "--socket".into(),
            "/tmp/codex.vsock".into(),
            command.into(),
            value.into(),
        ]
    }

    #[test]
    fn builds_only_named_vibe_operations() {
        let (_, port, op, payload) = command(&args("submit-task", "hello")).unwrap();
        assert_eq!(port, 7000);
        assert_eq!(op, Operation::SubmitTask);
        assert_eq!(payload["task"], "hello");
        assert_eq!(
            command(&args("shell", "id")).unwrap_err(),
            "command is not an allowlisted Vibe operation"
        );
    }

    #[test]
    fn rejects_bad_endpoint_options_and_oversized_argument() {
        assert!(command(&["--port".into(), "22".into(), "status".into(), "x".into()]).is_err());
        assert!(command(&[
            "--socket".into(),
            "/tmp/codex.vsock".into(),
            "status".into(),
            "x".into(),
            "extra".into()
        ])
        .is_err());
        let huge = "x".repeat(alcatraz_guest::MAX_PAYLOAD + 1);
        assert!(command(&args("status", &huge)).is_err());
    }
}
