//! Local endpoint proof: two independent Unix sockets model the two Vibe/Codex
//! directions. The production transport can replace these paths with the
//! Firecracker-vsock guest endpoints without changing protocol/policy code.
use alcatraz_guest::{authorize_channel, decode, digest, encode, Frame, Operation};
use serde_json::json;
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
    thread,
};

fn make_frame(channel: &str, id: &str, op: Operation, payload: serde_json::Value) -> Frame {
    let bytes = serde_json::to_vec(&payload).expect("payload json");
    Frame {
        version: 1,
        channel: channel.into(),
        request_id: id.into(),
        operation: op,
        nonce: format!("nonce-{id}"),
        payload_sha256: digest(&bytes),
        payload,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir =
        PathBuf::from(std::env::temp_dir()).join(format!("alcatraz-demo-{}", std::process::id()));
    fs::create_dir_all(&dir)?;
    let to_codex = dir.join("vibe-to-codex.sock");
    let to_vibe = dir.join("codex-to-vibe.sock");
    let in_listener = UnixListener::bind(&to_codex)?;
    let out_listener = UnixListener::bind(&to_vibe)?;
    let server = thread::spawn(move || -> Result<(), String> {
        let (input, _) = in_listener.accept().map_err(|e| e.to_string())?;
        let (mut output, _) = out_listener.accept().map_err(|e| e.to_string())?;
        let mut line = String::new();
        BufReader::new(input)
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
        let request = decode(line.as_bytes()).map_err(|e| e.to_string())?;
        authorize_channel(&request.channel, request.operation)?;
        let payload = json!({"accepted":true,"request_id":request.request_id,"source":"codex"});
        let response = make_frame("codex-to-vibe", "response-1", Operation::Result, payload);
        output
            .write_all(&encode(&response).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        Ok(())
    });
    let mut request = UnixStream::connect(&to_codex)?;
    let mut response = UnixStream::connect(&to_vibe)?;
    let frame = make_frame(
        "vibe-to-codex",
        "task-1",
        Operation::SubmitTask,
        json!({"task":"demo"}),
    );
    request.write_all(&encode(&frame)?)?;
    let mut line = String::new();
    BufReader::new(&mut response).read_line(&mut line)?;
    let result = decode(line.as_bytes())?;
    authorize_channel(&result.channel, result.operation)?;
    println!(
        "vibe->codex request accepted; codex->vibe result received: {}",
        result.payload
    );
    server
        .join()
        .map_err(|_| "server thread panicked")?
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    fs::remove_file(to_codex).ok();
    fs::remove_file(to_vibe).ok();
    fs::remove_dir(dir).ok();
    Ok(())
}
