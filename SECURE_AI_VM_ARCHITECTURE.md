# AlcatrazVM — Direct Firecracker Architecture

Status: implemented and integration-tested; production Jailer verification
requires a host that permits `unshare(CLONE_NEWNS)`.

AlcatrazVM uses Firecracker directly under KVM. Nomad and other schedulers are
out of scope because they introduce additional orchestration and networking
machinery that is not required for this single-host isolation boundary.

## Security objective

Keep the host, the Codex VM, and the three ancillary VMs distinct:

```text
                         host browser
                              │
                     loopback-only ingress
                              │
                     ┌─────────────────┐
                     │ Vibe Kanban VM  │
                     └────────┬────────┘
                              ║ two vsock channels
                              ▼
                     ┌─────────────────┐
                     │    Codex VM      │
                     │ API key here     │
                     └───┬─────┬─────┬──┘
                         ║     ║     ║
                      vsock  vsock  vsock
                         ║     ║     ║
                     ┌───▼─┐ ┌─▼──┐ ┌▼────┐
                     │ Git │ │Build│ │future│
                     │ VM  │ │ VM  │ │role  │
                     └─────┘ └─────┘ └─────┘
```

The intended first deployment has four VMs: Vibe Kanban, Codex, Git, and
Build. Codex is the only VM allowed to communicate with every ancillary VM.
Vibe, Git, and Build have no direct path to one another. Build has no network
interface and no default route.

The host remains trusted. A compromise of the host kernel, KVM, Firecracker,
the launch controller, or the image-verification path invalidates the model.

PCI transport is prohibited. The launcher never passes `--enable-pci`, every
role configuration uses virtio-MMIO devices with `pci=off` in the guest boot
arguments, and startup verification rejects PCI configuration. This is also a
defense-in-depth response to CVE-2026-5747: the affected Firecracker
virtio-PCI transport versions are 1.13.0 through 1.14.3 and 1.15.0, while the
advisory states that legacy MMIO is not affected. The pinned runtime must be
at least Firecracker 1.15.1.

## Firecracker launch model

Each VM is launched directly using a pinned Firecracker binary and a separate
JSON configuration file. A small reviewed host shell/tooling layer may invoke
the Firecracker local API, but there is no host-side Rust launcher. Host launch
scripts must not expose arbitrary shell or arbitrary Firecracker arguments to
guests.

The repository should eventually contain:

```text
config/firecracker/vibe-kanban.json
config/firecracker/codex.json
config/firecracker/git.json
config/firecracker/build.json
```

Every configuration must specify the same verified base kernel and common base
rootfs lineage, fixed memory and vCPU limits, a serial log destination, and
only the devices needed by that role. Each VM receives its own copy-on-write
or cloned writable rootfs; the base image is never modified in place and is
never mounted read/write by multiple VMs.

During first initialization, a role VM may temporarily receive a narrowly
controlled network interface so its pinned provisioning script can install the
role's software. After provisioning, the network device and route are removed,
the provisioning state is sealed, and the VM is restarted before it enters
normal operation. Build must follow the same initialization process, then run
with no network device and no default route.

This simplifies image maintenance without treating initialization as trusted:
the provisioning scripts, package sources, package versions, lockfiles,
checksums, and resulting manifest must all be pinned and recorded. A networked
initialization VM is not allowed to access credentials, host files, another VM,
or the production vsock services.

Illustrative Firecracker configuration shape:

```json
{
  "boot-source": {
    "kernel_image_path": "/var/lib/alcatraz/images/common-vmlinux",
    "boot_args": "console=ttyS0 reboot=k panic=1 pci=off"
  },
  "drives": [
    {
      "drive_id": "rootfs",
      "path_on_host": "/var/lib/alcatraz/images/build-rootfs.ext4",
      "is_root_device": true,
      "is_read_only": false
    }
  ],
  "machine-config": {
    "vcpu_count": 1,
    "mem_size_mib": 4096,
    "ht_enabled": false
  },
  "logger": {
    "log_path": "/var/lib/alcatraz/logs/build.log",
    "level": "Info",
    "show_level": true,
    "show_log_origin": true
  }
}
```

The example is a schema illustration, not a ready-to-run Build image. The
host launch tooling must apply the configuration through Firecracker's local API,
verify the resulting process and devices, and place each process in its own
jailer/chroot, UID/GID, cgroup, and seccomp policy where supported.

### Boot-artifact contract

Build one minimal common Debian rootfs with `debootstrap --variant=minbase`
from a pinned release and mirror snapshot. Firecracker boots a verified Linux
kernel and that rootfs; it does not boot an ISO. Record the release, snapshot,
package manifest, kernel SHA-256, rootfs SHA-256, provisioning-script digest,
and final filesystem digest. The base rootfs is immutable and each role gets a
fresh clone before provisioning.

## Vsock communication

Each Codex-to-ancillary relationship uses two independent channels:

```text
Codex VM ── codex-to-vibe.vsock ──► Vibe VM
Vibe VM  ── vibe-to-codex.vsock ──► Codex VM

Codex VM ── codex-to-git.vsock ──► Git VM
Git VM   ── git-to-codex.vsock ──► Codex VM

Codex VM ── codex-to-build.vsock ──► Build VM
Build VM ── build-to-codex.vsock ──► Codex VM
```

The two directions must use separate listening endpoints, queues, ownership,
and sequence spaces. A receiver must never write into a buffer owned by the
sender. Messages are framed and bounded; a stream is not treated as a trusted
message boundary.

The exact Firecracker vsock topology is implemented with a narrow host broker:
Firecracker exposes a guest vsock device and host-side Unix socket paths, and
the broker deliberately connects only the declared endpoints. For a
host-initiated connection it sends `CONNECT <port>\\n` and requires an
`OK <host-port>\\n` response. For a guest-initiated connection it listens on
the Firecracker-provided `<uds_path>_<port>` socket. The Vibe-to-Codex proof
uses fixed endpoint paths and forwards bytes only after the Codex listener has
accepted port 7000; it does not rely on an unrestricted CID-only transport.
The implementation must not assume that assigning CIDs alone creates a
guest-to-guest connection.

## Rust communication controller

The communication layer will be implemented inside the VMs using small Rust
guest-side clients and brokers. There is no host-side Rust launcher or host
side Rust message broker. Host startup remains a narrow Firecracker API/script
operation; the guest processes establish and enforce the application protocol
after boot.

### Components

```text
                   guest-side Rust communication layer
       ┌─────────────────────────┼─────────────────────────┐
       │                         │                         │
 Vibe adapter              Codex broker              Git/Build clients
       │                         │                         │
       └──────────── six directional vsock channels ──────┘
```

The guest-side Rust code has five logical modules:

1. `config` — loads only the guest's fixed local protocol configuration and
   rejects unsupported options;
2. `vsock` — connects to and supervises the guest's assigned vsock endpoints;
3. `protocol` — frames, bounds, parses, validates, and serializes JSON Lines;
4. `policy` — authorizes `(source_role, destination_role, direction,
   operation)` tuples and applies quotas, deadlines, and replay checks.
5. `lifecycle` — handles guest-side reconnect, shutdown, and protocol session
   state without starting or controlling Firecracker.

Firecracker binary paths, image paths, host Unix socket paths, and guest CIDs
are host launch configuration, not guest-provided values. Guest Rust code must
not accept these values from a Vibe request, a Codex model response, or guest
JSON.

### Endpoint wiring

Each Firecracker VM gets a vsock device configured with a unique guest CID.
The Firecracker launch configuration exposes the host-side Unix socket
endpoints needed by the guest vsock devices. The guest Rust processes connect
to their assigned endpoints and create two logical channels with independent
protocol state:

```text
  host listener A ──► Codex guest ──► Vibe guest
  host listener B ◄── Codex guest ◄── Vibe guest

  host listener C ──► Codex guest ──► Build guest
  host listener D ◄── Codex guest ◄── Build guest
```

The exact direction of a host Unix socket connection depends on the selected
Firecracker vsock configuration and guest transport implementation. The Rust
guest clients and host launch tooling must establish this experimentally with a two-VM echo test, then
encode the result in the manifest and tests. It must never infer connectivity
from CIDs alone.

Guest clients do not bridge arbitrary sockets. Each client connects only to its
declared endpoint and drops any session that does not identify the expected
peer role and protocol version during the handshake.

### Message lifecycle

For every message, the Rust path is:

```text
bytes from vsock
  → bounded frame reader
  → size and UTF-8 checks
  → JSON parse into typed Rust enum
  → channel/role/operation authorization
  → nonce, request, deadline, and digest checks
  → bounded policy action or rejection
  → typed response on the opposite directional channel
```

JSON must be parsed into typed Rust structures rather than passed around as
unvalidated generic maps. Recommended implementation types include a tagged
`Operation` enum, role-specific request structs, a bounded `RequestId`, and a
`Response` enum. Unknown operation names, unknown fields, oversized strings,
duplicate request IDs, stale nonces, and invalid state transitions are errors.

The initial frame limit should be conservative and explicit, for example a
small control-record limit plus a separate bounded artifact-transfer path.
Large Build inputs and outputs must be transferred as digest-addressed chunks
with a declared total size, per-chunk limit, sequence number, and final hash;
they must not be placed in one unbounded JSON string.

### Directional queues and ownership

Each channel has its own bounded receive queue, send queue, sequence space,
backpressure state, and cancellation token. A channel task owns its buffers and
is the only task allowed to mutate them. The broker passes immutable validated
records between tasks.

This prevents a response from overwriting an in-flight request and makes the
two-direction design meaningful. It does not by itself make data trusted:
Build responses remain attacker-controlled until the policy validator creates
a safe result record.

### Authorization matrix

The policy module should encode the topology as data, not scattered conditionals:

| Source | Destination | Allowed operations |
| --- | --- | --- |
| Vibe | Codex | submit task, status query, operator-review request |
| Codex | Vibe | bounded status, result, failure |
| Codex | Git | status, fetch, scoped push |
| Git | Codex | status/result/error; never the Git key |
| Codex | Build | send bundle, check, test, run, reset |
| Build | Codex | bounded result, artifact digest, error, incident enum |
| Vibe/Git/Build | one another | none |

The guest broker should reject a message based on its authenticated channel before
deserializing operation-specific payloads. A guest cannot claim to be Codex by
putting `"source": "codex"` in JSON; source identity comes from the endpoint
that accepted the connection and the guest session state.

### Guest-side clients

Each VM gets a minimal Rust or small native client appropriate to its image.
The client connects only to its assigned vsock service and implements the
typed protocol. The guest client must not expose the host socket, accept
additional listeners, or reinterpret a typed operation as arbitrary shell text.

The Build client maps `check`, `test`, and `run` to fixed local executors with
resource limits. The Git client maps operations to a restricted helper that
controls repository, remote, branch, and force-push policy. The Vibe adapter
maps the UI's task lifecycle to `submit_task`, `status`, `result`, and
`operator_review` records.

### Rust failure handling

Each guest communication client must treat EOF, malformed frames, guest restart, timeout,
protocol-version mismatch, digest mismatch, and unexpected channel closure as
session failures. It should close only the affected session, record a safe
event, and require an explicit lifecycle decision before reconnecting. It must
not silently reconnect a changed VM to an old channel or reuse a nonce space.

On reset, the host launch tooling stops the target Firecracker process, closes
its configured sockets, removes only that VM's runtime directory, verifies the
process is gone, and launches a fresh instance from the verified image. Guest
clients close their sessions and discard nonce state. Build reset must be
independently testable without restarting Codex.

### Rust implementation sequence

1. Define protocol structs and rejection tests without Firecracker.
2. Implement a pair of local Unix-socket endpoints using the same framing and
   policy code.
3. Boot two minimal Firecracker VMs and prove one request/response vsock pair.
4. Add the second direction and prove independent queues and sequence spaces.
5. Add the Codex/Build operation matrix and Build reset.
6. Add Git policy and credential isolation.
7. Add the Vibe adapter and loopback-only host ingress.
8. Add restart, replay, oversized-message, malformed-message, and denied-path
   integration tests.

The Rust crate should remain small and dependency-minimized. Every dependency
used for framing, JSON, async I/O, hashing, or Unix permissions must be pinned,
audited, and included in the image/build provenance record.

## Debugging and VM logs

Every VM must have observable startup and runtime output. The direct launcher
is `scripts/alcatraz-vm`. It accepts only the allowlisted roles `vibe-kanban`,
`codex`, `git`, and `build`; it does not accept arbitrary Firecracker flags or
guest-provided commands.

For each role it creates a private runtime directory containing:

- `serial.log` — guest console output and boot failures;
- `firecracker.stderr.log` — Firecracker startup, API, device, and runtime
  errors;
- `firecracker.api.sock` — the local Firecracker API socket;
- `pid` — the supervised Firecracker process ID.

Examples:

```bash
scripts/alcatraz-vm start build config/firecracker/build.json
scripts/alcatraz-vm status build
scripts/alcatraz-vm logs build
scripts/alcatraz-vm logs build follow
scripts/alcatraz-vm stop build
```

The launcher captures logs even when a VM never reaches a login prompt. Guest
communication clients must also emit bounded, structured protocol events to
their serial logs: connection established, handshake accepted/rejected,
operation accepted/rejected, timeout, malformed frame, peer restart, and
reset. They must never log API keys, Git keys, raw credentials, or unrestricted
Build output. Detailed attacker-controlled output remains in a separately
quarantined artifact store.

The acceptance tests must inspect both `serial.log` and
`firecracker.stderr.log`; a process exit without a clear error is a failed
diagnostic result, not a successful shutdown.

## JSON protocol

The initial protocol is JSON Lines over the directional vsock channels. JSON is
an interchange format, not a security boundary. Every record requires:

```json
{
  "version": 1,
  "channel": "codex-to-build",
  "request_id": "opaque-bounded-id",
  "operation": "check",
  "nonce": "opaque-bounded-value",
  "payload_sha256": "digest-of-bounded-payload",
  "payload": {}
}
```

Required protocol properties:

- maximum record and payload sizes;
- strict schema validation and unknown-field rejection;
- request IDs, nonces, deadlines, and replay detection;
- per-channel allowlists and rate limits;
- no arbitrary host paths, URLs, environment variables, or shell text from
  Vibe or other ancillary VMs;
- structured errors without raw host diagnostics;
- audit records containing operation, role, sizes, hashes, and outcome;
- raw Build output quarantined until a validator creates a bounded result.

The user-facing phrase “send a Bash command” must be translated into named
operations such as `build`, `test`, `run`, `git_status`, or `reset`. The
protocol must not become a general remote shell.

## Role boundaries

### Vibe Kanban VM

Vibe Kanban runs Node.js/npm and the browser-facing service. It is treated as
potentially compromised by a malicious package or lifecycle script. It has no
OpenAI key, Git key, host mount, or direct socket to Git or Build. It can submit
only typed job requests on its Vibe-to-Codex channel.

### Codex VM

Codex holds runtime-injected OpenAI credentials and runs the agent/broker. It
may communicate with Vibe, Git, and Build using separate typed channels. It
does not receive arbitrary raw Build logs, and it cannot use the Git key as a
filesystem secret.

### Git VM

The Git identity exists only in the Git VM. Git operations are destination,
repository, branch, and operation limited. The VM must reject force pushes and
unauthorized remotes by policy.

### Build VM

Build has no network device, no default route, no API credentials, and no Git
identity. It receives bounded source bundles and returns bounded results. Its
rootfs is disposable; reset destroys the VM and recreates it from the verified
image.

## Common base image and initialization

All four VMs start from one immutable, digest-pinned base image. The image
contains only the common operating-system layer, the guest communication
client, diagnostics, and the minimum tools needed to complete provisioning.
Role differences are created by scripts and configuration during a controlled
initialization phase:

```text
verified common base image
            │
            ├── clone → Vibe VM → temporary provisioning network → seal
            ├── clone → Codex VM → temporary provisioning network → seal
            ├── clone → Git VM   → temporary provisioning network → seal
            └── clone → Build VM → temporary provisioning network → seal
                                                               │
                                              remove NIC/routes, restart
```

Each role has a separate, reviewed provisioning script, for example:

```text
provision/vibe-kanban.sh
provision/codex.sh
provision/git.sh
provision/build.sh
```

The scripts install only pinned software and write a machine-readable
provisioning manifest containing package versions, source URLs, lockfile
digests, and final filesystem hashes. They must run as an unprivileged role
user wherever possible and must not receive API keys, Git keys, host mounts, or
production vsock peers.

Initialization is a build step, not a live operating mode. Before sealing a
VM, the launcher must verify that:

1. the expected role software and guest protocol client are present;
2. package and artifact verification succeeded;
3. no unexpected listening services, routes, users, SSH keys, or credentials
   were created;
4. the provisioning manifest was written and its digest recorded;
5. the temporary network interface, default route, resolver state, and
   provisioning-only sockets can be removed;
6. the VM can reboot and start in offline mode.

The normal launcher must refuse to start a sealed VM if its network-removal
marker or provisioning manifest is missing. Reinitialization creates a fresh
clone from the common base image; it does not mutate a running production VM.

The convenience of one common image must not become a shared mutable rootfs or
an excuse to run unpinned `curl | sh`, `npm install`, or package-manager
commands against floating versions. Network access exists only for the
explicit initialization window and is removed before secrets or production
communication are enabled.

## Vibe Kanban and Codex integration

The integration must be designed from observed Vibe Kanban behavior rather
than guessed at the protocol. First identify the minimal events needed for:

1. creating or selecting a task;
2. submitting a Codex job;
3. streaming bounded status;
4. returning a result or failure;
5. requesting operator review.

The Vibe-facing adapter translates those events into the typed JSON protocol.
Vibe does not get a raw Codex socket, API key, arbitrary command channel, or
filesystem access. If the required integration cannot be cleanly separated,
the fallback is to co-locate only the narrow adapter with Codex—not to expose
Codex credentials to the full Node/npm process.

## Launch and lifecycle controller

The host launch scripts/configuration are responsible for:

1. verifying image and binary digests;
2. creating isolated runtime directories and Unix socket paths;
3. launching Firecracker processes with fixed JSON configuration;
4. wiring each directional vsock endpoint;
5. starting and stopping Firecracker processes;
6. deleting disposable VM state on reset;
7. recording safe lifecycle events without credentials or raw attacker data.

They must fail closed if an image, socket, CID, path, or configuration is
unexpected. They must not accept arbitrary command strings from the Vibe UI or
Codex model. Protocol authorization belongs to the guest Rust code, not to a
host-side Rust process.

## Acceptance sequence

1. Launch one no-network Firecracker VM from a verified kernel/rootfs pair.
2. Prove the VM can boot, stop, and reset cleanly.
3. Prove one bidirectional vsock pair with bounded JSON Lines.
4. Launch Codex and Build; prove `check` over the two Build channels.
5. Prove Build cannot see host files, credentials, or network interfaces.
6. Launch Git and prove one narrowly scoped status operation.
7. Launch Vibe Kanban and prove loopback-only health and task submission.
8. Prove Vibe cannot connect to Git or Build directly.
9. Prove malformed, oversized, replayed, and unauthorized messages fail closed.
10. Prove reset removes Build state, processes, sockets, and temporary files.

A successful Firecracker boot is not proof of isolation. Each denied path needs
a negative test, and every image/configuration change must be reviewed and
digest-pinned.

## Historical context

The former seL4/CAmkES/QEMU design and its postmortem remain useful background,
but they are no longer the implementation target. Their bring-up logs must not
be presented as validation of this direct Firecracker architecture.
