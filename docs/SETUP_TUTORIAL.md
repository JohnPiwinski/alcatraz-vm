# AlcatrazVM setup tutorial

This tutorial builds and exercises the minimal Debian/Firecracker system from
a clean host. It is intentionally explicit: each command has a corresponding
verification step, and a green process boot alone is not treated as proof of
isolation.

## 1. Understand the layout

AlcatrazVM has four role VMs:

```text
host browser ── loopback/vsock ── VibeKanban
                                      │ typed vsock
                                      ▼
                                   Codex
                                  ╱     ╲
                             typed       typed
                            vsock         vsock
                           Git VM       Build VM
```

Only Codex is authorized to communicate with all ancillary roles. The
protocol library rejects malformed, oversized, replayed, digest-invalid, and
topologically unauthorized records. PCI transport is disabled in every
configuration; Firecracker uses virtio-MMIO.

The VM boots a kernel plus an ext4 rootfs, not an ISO. Podman supplies the
userland used to construct the rootfs; it does not supply the VM kernel.

## 2. Check host prerequisites

```bash
test -r /dev/kvm
podman --version
jq --version
debugfs -V
mkfs.ext4 -V
git --version
cargo --version
/home/jpwin/firecracker/firecracker-v1.16.1-x86_64 --version
/home/jpwin/firecracker/jailer-v1.16.1-x86_64 --version
```

The production launcher needs root or equivalent capability to create the
Jailer mount namespace and chroot, then drop to the configured VM UID/GID. A
restricted development container may be able to run Firecracker directly with
KVM while still rejecting Jailer’s `unshare(CLONE_NEWNS)` call. That is an
environment limitation, not evidence that the jailed path is safe to skip.

## 3. Build the minimal Debian artifacts

The clean role Containerfiles use a digest-pinned Debian base. The `ms`
container is not exported or used as a parent image. Use Podman to build each
role:

```bash
mkdir -p .image-build/roles
for role in vibe-kanban codex git build; do
  case "$role" in vibe-kanban) stem=vibe ;; *) stem="$role" ;; esac
  scripts/build-role-rootfs-podman "$role" \
    "$PWD/.image-build/$stem-rootfs.tar"
  scripts/tar-to-ext4 "$PWD/.image-build/$stem-rootfs.tar" \
    "$PWD/.image-build/roles/alcatraz-$stem-rootfs.ext4"
done
```

The role `vibe-kanban` is the browser-facing service. Its builder starts the
clean container, waits for its internal port 3000 to become healthy, copies
the pinned runtime into the image, and exports the warmed image. This avoids
depending on a host-installed VibeKanban binary.

A Firecracker kernel and selective initramfs are required:

```bash
scripts/make-root-initramfs "$PWD/.image-build/vibe-root-initramfs.cpio.gz"
```

The kernel must be stored as `.image-build/debian-vmlinux` and accompanied by
its SHA-256 file. Build or obtain it through the repository’s approved kernel
pipeline; do not substitute an arbitrary host kernel.

## 4. Inject fixed role executors and seal images

Build the Rust binaries and inject only the named helpers:

```bash
cargo build --bins
for role in codex git build; do
  scripts/inject-role-executors "$role" \
    "$PWD/.image-build/roles/alcatraz-$role-rootfs.ext4"
done
```

The Vibe HTTP proxy is injected by the Vibe Firecracker smoke path. For a
production image, include the reviewed proxy in the final image before
sealing. Then create the host-side contract for every final image:

```bash
for role in codex git build; do
  scripts/seal-role-image "$role" \
    "$PWD/.image-build/roles/alcatraz-$role-rootfs.ext4" \
    "$PWD/.image-build/debian-vmlinux"
done
scripts/seal-role-image vibe-kanban \
  "$PWD/.image-build/roles/alcatraz-vibe-rootfs.ext4" \
  "$PWD/.image-build/debian-vmlinux"
```

The seal records the role, rootfs digest, kernel digest, provisioning-script
digest, network-removal state, credential state, and PCI state. Verify it:

```bash
scripts/verify-image-contract ROLE ROOTFS_EXT4 KERNEL
```

Any later image mutation invalidates the digest and must be followed by a new
seal. Never share a writable rootfs between VMs.

## 5. Run the acceptance suite

```bash
scripts/verify-role-boundaries
scripts/integration-smoke
```

Expected evidence includes:

- `11 passed` Rust protocol tests.
- `virtio MMIO only` from `verify-no-pci`.
- A real Firecracker vsock handshake and bounded echo.
- A two-VM Vibe→host broker→Codex path.
- `host-http ... HTTP 200`, proving the host reached VibeKanban in its VM.
- `vibe->codex request accepted`, proving the typed channel round trip.
- `git: clone passed; SSH key injected ... 0700/0600 permissions`.
- Firecracker Cargo `check` and Git `status` executor successes.
- `cargo: cargo test passed`.

The test rootfs copies are disposable. The scripts clean up their Firecracker
processes and sockets; inspect the retained serial/stderr logs if a test fails.

## 6. Launch a production role

The checked-in configs use `/var/lib/alcatraz/images`, `/var/lib/alcatraz/run`,
and `/var/lib/alcatraz/logs`. Install the verified artifacts and sidecars at
those paths, or generate an equivalent deployment configuration through the
approved image installer. Then:

```bash
scripts/alcatraz-vm start build config/firecracker/build.json
scripts/alcatraz-vm status build
scripts/alcatraz-vm logs build
```

The launcher refuses:

- an unknown role or mismatched config;
- non-approved host paths;
- a missing or mismatched sealed manifest;
- a missing offline marker;
- a wrong guest CID;
- PCI configuration;
- an unavailable Jailer in its default mode.

For the current development environment only, an explicit unjailed probe is:

```bash
ALCATRAZ_LAUNCH_MODE=unjailed ALCATRAZ_UNSAFE_UNJAILED=1 \
  scripts/alcatraz-vm start build config/firecracker/build.json
```

Do not put credentials into an unjailed development VM.

## 7. Use the role APIs safely

The protocol is a bounded JSON Lines record. A Build request names `check`,
`test`, `run`, or `reset`; it never carries a shell command. A Git request
names a repository, an allowlisted remote, a branch, and one of the fixed
operations. The guest executors validate the frame channel and typed payload
before invoking the fixed `/usr/bin/cargo` or Git helper.

VibeKanban receives only the Vibe↔Codex typed adapter. It does not receive the
Codex API key, a raw Codex socket, a Git key, an arbitrary host path, or a
general remote shell.

## 8. Replace VibeKanban with the command-line emulator

The `vibe-codex-emulator` binary is a narrow substitute for the VibeKanban
adapter. It performs the fixed handshake and sends only `submit-task`,
`status`, or `operator-review` typed records:

```bash
cargo build --bin vibe-codex-emulator
vibe-codex-emulator --socket /var/lib/alcatraz/run/codex.vsock \
  submit-task 'Implement the sample project'
scripts/vibe-emulator-smoke
```

The emulator uses bounded reads, strict frame decoding, channel authorization,
and response-operation checks. It gives a future UI the same narrow interface
without granting a raw shell or credentials.

## 9. Run a real second Codex with subscriber authentication

OpenAI’s official documentation says Codex supports “Sign in with ChatGPT” for
subscription access and API-key login as a separate usage-based mode. On the
machine or VM that owns the second Codex process:

```bash
codex login
# choose: Sign in with ChatGPT
codex login status
scripts/codex-subscriber-smoke
```

The browser/device sign-in is user-mediated. Keep the resulting credential
store inside the Codex VM and inject it only at runtime; never bake it into a
rootfs or expose it to VibeKanban, Git, or Build. The reproducible smoke
creates a disposable Rust project, asks the second Codex process to complete
it, and verifies Cargo tests. For fully unattended service use, use a
separately approved API/service identity rather than treating a ChatGPT
subscription session as an API key.

To exercise the complete replacement path—emulated Vibe command, authenticated
Codex process, edited sample project, and post-agent Cargo verification—run:

```bash
scripts/vibe-emulator-codex-sample
```

This is intentionally opt-in because it consumes one Codex subscriber run;
the cheaper protocol-only emulator test remains part of the regular suite. The
sample runner defaults to `ALCATRAZ_AGENT=codex`; `ALCATRAZ_AGENT=opencode` is
also supported when that executable is installed, while unknown providers are
rejected.

## 10. Reset and recover

```bash
scripts/alcatraz-vm stop build || true
scripts/alcatraz-vm reset build
```

### Production Jailer check

The normal launcher defaults to Jailer mode and therefore needs root for the
chroot, mount namespace, UID/GID drop, and resource limits. On a host that
permits mount namespaces, run:

```bash
sudo /home/jpwin/ai_coding/projects/alcatraz_vm/scripts/alcatraz-vm start build \
  /home/jpwin/ai_coding/projects/alcatraz_vm/config/firecracker/build.json
sudo /home/jpwin/ai_coding/projects/alcatraz_vm/scripts/alcatraz-vm status build
sudo /home/jpwin/ai_coding/projects/alcatraz_vm/scripts/alcatraz-vm stop build
```

If the host reports `unshare(CLONE_NEWNS): Operation not permitted`, its
container or service policy is blocking Jailer; direct Firecracker/KVM smoke
tests can still run, but this production-launch check remains unverified.

`/dev/kvm` access and Jailer privilege are separate. If the device uses the
`kvm` group, configure membership once and refresh the login session:

```bash
sudo usermod -aG kvm "$USER"
newgrp kvm
test -r /dev/kvm && test -w /dev/kvm
```

This can remove root for explicitly unjailed development probes, but it cannot
replace Jailer’s mount namespace and chroot privileges. Do not make
Firecracker setuid or grant the operator broad `CAP_SYS_ADMIN`. A production
installation that avoids repeated password prompts should use a root-owned,
immutable launcher and narrowly argument-matched `sudoers` entries, or a
root-managed service with an authenticated control interface.

Reset removes the exact role runtime directory, including the PID file,
sockets, staged copies, and logs. Recreate the VM from the verified sealed
image. If a role exits unexpectedly, inspect both logs before restarting:

```bash
scripts/alcatraz-vm logs build
```

Never repair a running production rootfs in place.

## 11. Troubleshooting

### `/dev/kvm` is present but boot fails

Check that the Firecracker process can open it, that the kernel and initramfs
paths are readable, and inspect `firecracker.stderr.log`. A present device node
alone is not sufficient.

### Jailer reports `Operation not permitted`

The host/container lacks mount-namespace capability. Run the production
launcher on the host with root or an equivalent capability set. Do not treat
the unjailed opt-in as a production workaround.

### VibeKanban boots but host HTTP fails

Inspect `serial.log` for both `Main server on :3000` and
`ALCATRAZ_VIBE_VSOCK_HTTP_LISTENING port=3000`. Confirm the host connects to the
Firecracker UDS and sends `CONNECT 3000`; the guest proxy intentionally exposes
only that fixed port and loopback backend.

### Cargo fails in the Build VM

Confirm the image contains `/usr/bin/cargo`, the injected executor is current,
the fixture/workspace is inside the Build root, and the guest proxy is not
ignoring `SIGCHLD`. The fixed launcher reaps children so Cargo’s status can be
collected correctly.

### Image verification fails

The image was changed after sealing or the kernel/rootfs pair does not match.
Rebuild or deliberately reseal the final artifact, then rerun the contract
verifier. Do not bypass the check by deleting sidecars.
