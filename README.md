# AlcatrazVM

AlcatrazVM is a direct Firecracker/KVM isolation layout for four fixed roles:
VibeKanban, Codex, Git, and Build. The roles communicate through explicitly
declared vsock channels and bounded, typed JSON Lines records. VibeKanban has
loopback-only browser ingress; Codex is the only role that may address Git and
Build; Build has no network device.

The design and security requirements are documented in
[`SECURE_AI_VM_ARCHITECTURE.md`](SECURE_AI_VM_ARCHITECTURE.md). The step-by-step
setup guide is [`docs/SETUP_TUTORIAL.md`](docs/SETUP_TUTORIAL.md), and a
browser-readable version is [`docs/index.html`](docs/index.html).

## Prerequisites

- Linux x86-64 with `/dev/kvm` readable by the launcher user.
- Podman for building the clean Debian role images. The existing `ms` image is
  not used as a base.
- `jq`, `curl`, `debugfs`, `mkfs.ext4`, `cc`, `git`, and Rust/Cargo.
- The pinned Firecracker v1.16.1 bundle, including `firecracker` and `jailer`.
- Root or equivalent mount-namespace capability for the production Jailer
  path. Unjailed execution is available only as an explicit development opt-in.

## Build the artifacts

The repository uses a minimal Debian userland and does not boot an ISO. Build
the kernel/rootfs pair and role images in a writable build directory:

```bash
mkdir -p .image-build/roles
# Obtain/build .image-build/debian-vmlinux and the root-initramfs first.
scripts/build-role-rootfs-podman vibe-kanban "$PWD/.image-build/vibe.tar"
scripts/tar-to-ext4 "$PWD/.image-build/vibe.tar" "$PWD/.image-build/roles/alcatraz-vibe-rootfs.ext4"
scripts/seal-role-image vibe-kanban \
  "$PWD/.image-build/roles/alcatraz-vibe-rootfs.ext4" \
  "$PWD/.image-build/debian-vmlinux"
```

For a one-step role build, use:

```bash
scripts/build-sealed-role-rootfs-podman ROLE OUTPUT_EXT4 KERNEL
```

The role names are `vibe-kanban`, `codex`, `git`, and `build`. Inject the
reviewed fixed executors into Codex/Git/Build images with
`scripts/inject-role-executors` and re-seal the resulting filesystem. Always
run `scripts/verify-image-contract` after the final mutation.

## Verify and launch

Check PCI policy and image contracts:

```bash
scripts/verify-no-pci
scripts/verify-role-boundaries
scripts/verify-image-contract ROLE ROOTFS_EXT4 KERNEL
```

Production launch is jailed and accepts only fixed role/config combinations:

```bash
scripts/alcatraz-vm start build config/firecracker/build.json
scripts/alcatraz-vm status build
scripts/alcatraz-vm logs build
scripts/alcatraz-vm stop build
scripts/alcatraz-vm reset build
```

The launcher verifies the role, configuration paths, guest CID, sealed image
manifest, and PCI/MMIO policy before starting. It then stages a private copy
of the kernel and rootfs and invokes the bundled Jailer with a per-role
chroot, UID/GID drop, PID namespace, and resource limit. To run only a local
development probe without Jailer, both safeguards must be explicit:

```bash
ALCATRAZ_LAUNCH_MODE=unjailed ALCATRAZ_UNSAFE_UNJAILED=1 \
  scripts/alcatraz-vm start ROLE config/firecracker/ROLE.json
```

Do not use that mode for production or credential-bearing VMs.

## Acceptance tests

Run the complete suite with:

```bash
scripts/integration-smoke
```

It covers protocol tests, sealed image contracts, PCI exclusion, role-boundary
negative checks, real Firecracker vsock tests, VibeKanban HTTP access from the host, Vibe↔Codex
communication, Git SSH-key injection and clone, typed Git/Build executors,
reset behavior, and Cargo/Git checks. The direct role tests can be run alone:

```bash
scripts/firecracker-role-executor-smoke build
scripts/firecracker-role-executor-smoke git
scripts/firecracker-vibe-smoke
scripts/git-ssh-smoke
scripts/vibe-emulator-smoke
```

VibeKanban’s separate clean Podman service, when available, is checked at
`http://127.0.0.1:3030/health`. The VM proof uses Firecracker vsock and does
not expose a guest network interface.

To exercise a second Codex CLI instance with a ChatGPT subscription:

```bash
codex login                 # choose “Sign in with ChatGPT”
scripts/codex-subscriber-smoke
```

This uses the CLI’s subscriber session, not `OPENAI_API_KEY`. See the
[official OpenAI authentication documentation](https://learn.chatgpt.com/docs/auth).
Keep the Codex
credential store private to the Codex VM; never copy `~/.codex/auth.json` into
VibeKanban, Git, or Build. For unattended deployment, use an approved
API/service identity if permitted; a ChatGPT subscription is not a general
purpose API key.

The strongest end-to-end proof is the opt-in authenticated path:

```bash
scripts/vibe-emulator-codex-sample
```

It sends a Vibe-shaped task through the emulator to a second Codex CLI, then
verifies that Codex completed the disposable sample and that Cargo passes. The
provider is configurable only through the fixed allowlist:
`ALCATRAZ_AGENT=codex` (the tested default) or `ALCATRAZ_AGENT=opencode` when
the `opencode` executable is installed. Unknown providers are rejected.

The acceptance requirements and their authoritative checks are summarized in
[`docs/VERIFICATION_MATRIX.md`](docs/VERIFICATION_MATRIX.md).

To perform the one host-dependent production check, run the launcher as root
so Jailer can create its chroot and mount namespace:

```bash
sudo /home/jpwin/ai_coding/projects/alcatraz_vm/scripts/alcatraz-vm start build \
  /home/jpwin/ai_coding/projects/alcatraz_vm/config/firecracker/build.json
sudo /home/jpwin/ai_coding/projects/alcatraz_vm/scripts/alcatraz-vm status build
sudo /home/jpwin/ai_coding/projects/alcatraz_vm/scripts/alcatraz-vm stop build
```

Do not set `ALCATRAZ_LAUNCH_MODE=unjailed`; that mode is an explicit
development escape hatch and is guarded by `ALCATRAZ_UNSAFE_UNJAILED=1`.

### KVM group versus Jailer privileges

If `/dev/kvm` is owned by group `kvm`, add the operator to that group once as
root, then log out and back in (or run `newgrp kvm` in a temporary shell):

```bash
sudo usermod -aG kvm "$USER"
newgrp kvm
id -nG
test -r /dev/kvm && test -w /dev/kvm
```

The `kvm` group only grants access to the KVM device. It does not grant the
mount-namespace/chroot capability required by Jailer, and adding broad
`CAP_SYS_ADMIN` or a setuid Firecracker binary would weaken the isolation
boundary. For passwordless operator control, the safe deployment pattern is a
root-owned, immutable installation plus narrowly argument-matched `sudoers`
rules (or a root-managed service with an authenticated control interface).

## Security model and operations

- Never pass arbitrary Firecracker flags, shell commands, host paths, URLs, or
  environment variables through the protocol.
- Git requests are constrained to a repository below the Git root, an
  allowlisted SSH remote prefix, a named branch, and non-force operations.
- Build requests use named Cargo operations and an in-root workspace; Cargo is
  invoked offline with bounded output.
- SSH keys are created and retained only in the Git VM. They are never returned
  through the Codex channel.
- Build state is disposable. Use `reset build` to stop the VM and remove its
  runtime state before recreating it from a verified sealed image.
- Keep `serial.log` and `firecracker.stderr.log`; they are the first diagnostic
  artifacts after a failed boot. They must not contain credentials or raw
  unbounded attacker-controlled output.

See the tutorial for image provenance, provisioning, troubleshooting, and the
reason root/mount-namespace capability is needed by Jailer.
