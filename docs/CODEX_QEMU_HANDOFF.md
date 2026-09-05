# Codex/QEMU handoff

This file is a portable project summary for a new Codex CLI instance running
inside a QEMU guest. It intentionally contains no API keys, subscriber tokens,
SSH private keys, or host credential paths.

## Objective

Implement and verify the direct Firecracker architecture in
`SECURE_AI_VM_ARCHITECTURE.md`: four role VMs (VibeKanban, Codex, Git, Build),
typed bounded vsock protocols, no virtual PCI transport, isolated Git SSH
identity, Cargo execution, Vibe↔Codex communication, an emulator for Vibe
commands, and a real second-agent sample-project test.

## Current implementation

- Rust protocol and policy implementation is in `src/lib.rs`.
- Typed executors are `src/bin/build-executor.rs` and
  `src/bin/git-executor.rs`.
- The Vibe replacement CLI is `src/bin/vibe-codex-emulator.rs`.
- Firecracker role configs are in `config/firecracker/`; all use
  `pci=off`, `smt=false`, fixed CIDs, and virtio-MMIO devices.
- The constrained launcher is `scripts/alcatraz-vm`.
- Rootfs construction uses Podman and minimal Debian userland; Firecracker
  boots a kernel plus ext4 rootfs and an initramfs, not a netinst ISO.
- Documentation is in `README.md`, `docs/SETUP_TUTORIAL.md`,
  `docs/index.html`, and `docs/VERIFICATION_MATRIX.md`.

## Verified evidence

The full integration suite has passed with exit 0. It covers:

- 14 Rust tests and formatting;
- Firecracker/KVM vsock handshake and two-guest routing;
- actual VibeKanban rootfs boot and host HTTP 200;
- Vibe↔Codex typed request/result;
- emulator `submit-task`, `status`, and `operator-review`;
- a second authenticated Codex completing a disposable Rust sample;
- Git clone with an ed25519 key injected into an isolated VM home with 0700/
  0600 permissions;
- Cargo and Git executor checks;
- image sealing, role boundaries, reset, and PCI/MMIO verification.

The host’s first sudo Jailer run also proved that Jailer creates its chroot,
starts Firecracker, and exposes the API socket. Subsequent fixes added host
artifact mapping, `root=/dev/vda`, and the production rootfs initramfs.

## Remaining verification

Run the following on a host with root, `/dev/kvm`, and mount-namespace support:

```bash
sudo /home/jpwin/ai_coding/projects/alcatraz_vm/scripts/alcatraz-vm reset build
sudo /home/jpwin/ai_coding/projects/alcatraz_vm/scripts/alcatraz-vm start build /home/jpwin/ai_coding/projects/alcatraz_vm/config/firecracker/build.json
sudo /home/jpwin/ai_coding/projects/alcatraz_vm/scripts/alcatraz-vm status build
sudo /home/jpwin/ai_coding/projects/alcatraz_vm/scripts/alcatraz-vm logs build
```

The guest log must show successful rootfs handoff and the role-specific Build
init marker, not merely Firecracker’s API socket. Stop it afterward with:

```bash
sudo /home/jpwin/ai_coding/projects/alcatraz_vm/scripts/alcatraz-vm stop build
```

If the guest stops immediately, inspect `serial.log` first. Do not claim the
production path is complete without the role-specific marker.

## New Codex guest setup

Copy this repository into the QEMU guest (prefer a fresh copy rather than a
writable host-home mount), install the required toolchain, and authenticate
the Codex CLI inside the guest with `codex login`. A ChatGPT subscription login
is separate from an API key; never copy the host’s `~/.codex/auth.json` into
VibeKanban, Git, or Build.

Before nested Firecracker testing, verify:

```bash
id
test -r /dev/kvm && test -w /dev/kvm
codex login status
```

Root inside QEMU is not host root. Nested Firecracker requires QEMU nested KVM
support and `/dev/kvm` in the guest. Firejail is optional process sandboxing;
it does not provide virtualization and may block KVM or mount namespaces.
