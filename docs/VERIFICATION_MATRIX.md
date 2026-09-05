# AlcatrazVM verification matrix

This matrix maps the acceptance requirements in
`SECURE_AI_VM_ARCHITECTURE.md` to the checks that produce evidence. Run the
full suite from the repository root with:

```bash
scripts/integration-smoke
```

## Acceptance evidence

| Requirement | Evidence | Result |
| --- | --- | --- |
| No-network Firecracker boot | `scripts/firecracker-role-executor-smoke build` | Pass: Build boots and runs a typed Cargo check in Firecracker. |
| Stop/reset lifecycle | `scripts/alcatraz-vm reset build` in the integration suite | Pass: runtime state is removed and the next start requires verified images. |
| Bounded bidirectional vsock | `scripts/firecracker-vsock-smoke` | Pass: handshake and bounded echo succeed. |
| Independent two-VM routing | `scripts/firecracker-two-vsock-smoke` | Pass: Vibe guest → host broker → Codex guest succeeds. |
| Build operation policy | Rust tests and `scripts/firecracker-role-executor-smoke build` | Pass: typed check works; arbitrary shell text is not an operation. |
| Build isolation | `scripts/verify-role-boundaries` and Build config inspection | Pass: no NIC, default-network setup, or known credential paths. |
| Git policy and identity isolation | `scripts/git-ssh-smoke` and `scripts/firecracker-role-executor-smoke git` | Pass: clone succeeds with an injected key confined to the Git VM home; key permissions are 0700/0600. |
| VibeKanban VM and host ingress | `scripts/firecracker-vibe-smoke` | Pass: actual VibeKanban rootfs boots and host receives HTTP 200 through the fixed vsock proxy. |
| Vibe↔Codex communication | `scripts/verify-vibe-codex` | Pass: typed request and result traverse separate directional channels. |
| Vibe replacement interface | `scripts/vibe-emulator-smoke` | Pass: `submit-task`, `status`, and `operator-review` round trips; unknown operations are rejected. |
| Real agent completion | `scripts/vibe-emulator-codex-sample` | Pass: an authenticated second Codex CLI edits a disposable Rust project and Cargo tests pass afterward. |
| Subscriber authentication | `codex login status` and `scripts/codex-subscriber-smoke` | Pass on this host using “Sign in with ChatGPT”; no API key is required. |
| PCI vulnerability mitigation | `scripts/verify-no-pci` | Pass: Firecracker 1.16.1, no `--enable-pci`, no PCI config, and `pci=off` in every role. |
| Sealed image provenance | `scripts/verify-image-contract` for all four roles | Pass: rootfs/kernel/provisioning digests and offline markers verify. |
| Malformed, oversized, replayed, and unauthorized records | Rust unit tests | Pass: 14 tests, 0 failures. |
| Jailer/chroot production launch | `scripts/alcatraz-vm` jailed mode | Implemented, but not executable in this sandbox: the host denies `unshare(CLONE_NEWNS)` with `EPERM`. A host with root and mount-namespace capability is required for this final runtime check. |

## Agent selection

The authenticated sample runner defaults to `ALCATRAZ_AGENT=codex`. It also
accepts `ALCATRAZ_AGENT=opencode` when the `opencode` executable is installed;
unknown providers are rejected. The Codex path is the one verified by the
real sample-project test.

## Image format note

The architecture intentionally boots a verified Linux kernel plus a minimal
Debian ext4 rootfs. Firecracker does not boot a Debian installer ISO in the
production design, so a netinst ISO is neither required nor used.
