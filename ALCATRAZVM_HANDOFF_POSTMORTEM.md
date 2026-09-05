# AlcatrazVM handoff postmortem

This document records what was attempted in the AlcatrazVM work, what was
actually validated, what failed, and what must not be assumed by a future
session. It is intentionally candid. It is a handoff document, not a claim
that the system is production-ready.

No credentials, API keys, SSH keys, or private file contents are recorded
here.

## Executive summary

The project did not reach the originally desired finished system. A substantial
amount of host-side Rust, UI, policy, test, and seL4/CAmkES bring-up code was
created, but the most important deployment features were not completed:

- There is no production-ready Vibe Kanban service running inside the seL4
  guest and reachable through a reviewed host-only network path.
- There is no production Codex VM with a real runtime API-key provisioning
  flow.
- There is no real Git VM with a tested Git SSH-key provisioning and push flow.
- There is no fully network-isolated Build VM with the final toolchain and
  reset lifecycle proven under the final image.
- There is no completed, reviewed seL4 firewall/network composition.
- The larger Build source/result path still contains compatibility plumbing
  rather than being only the simple JSON/JSONL bulk path originally desired.
- x86 guest multiple-vCPU support is not available in upstream libvmm.

The work did produce useful bring-up evidence and reusable code, but the work
expanded far beyond the original small prototype. The main process failure was
that intermediate scaffolding and simulated/staged behavior were repeatedly
described as if they were close to the final architecture. They were not.

## Intended architecture as it evolved

The first design was a larger five-role system:

1. Vibe Kanban UI;
2. Codex;
3. Build;
4. dependency/package fetch;
5. research/web retrieval.

The user later simplified this to one common Linux image cloned into four
guests:

1. Kanban;
2. Codex;
3. Git;
4. Build.

The desired topology is a Codex-centered wheel. Codex may communicate with
Kanban, Git, and Build; the three spokes must not communicate directly with
one another. Kanban should be host-accessible after initialization. Codex may
access the web, but the Git SSH key must remain outside Codex. Build should
have no secrets and eventually no network after provisioning.

The desired implementation was intentionally modest:

- reuse the existing egui UI;
- use one common guest image;
- use upstream seL4/CAmkES or Microkit shared-memory connectors;
- use bounded JSON/JSONL records rather than inventing an elaborate wire
  protocol;
- inject role-specific startup scripts;
- make the real Kanban service work first;
- then prove file transfer, Git, Codex, and Build behavior end to end.

## What was attempted

### 1. V1 custom protocol and large Rust control layer

The first implementation created a host-side Rust model for frames, sequence
numbers, payload lengths, hashes, ownership, capability roles, audit records,
incident records, launcher state, workspace allocation, and control requests.
It also added low-level C bridge helpers for the seL4 side.

Why it became a problem:

- The host UI, audit model, launcher, policy checker, test harness, and
  reference protocol model were counted together as if they were the core
  inter-VM communication implementation.
- The fixed-frame protocol was a reference/compatibility contract, not a
  complete seL4 transport.
- It made the project appear to contain tens of thousands of lines for a job
  that should have been a small shared-memory connector plus a broker.
- Raw C memory operations caused understandable concern, but replacing every
  low-level boundary with Rust was not realistic because the upstream
  CAmkES/VMM interfaces and generated glue are C-oriented.

Important correction: the large Rust line count was not all necessary for
communication. Much of it was UI, policy, tests, documentation support,
launch lifecycle, schemas, host controls, and defensive reference code. A
future implementation should keep the seL4 transport narrow and avoid
building a general-purpose host orchestration framework before the guest
workflow works.

### 2. Reusing the egui console and adding a launcher

The existing native egui application was reused. It gained a grayscale
topology diagram with blue as the active/data accent, a launcher view, role
selection, lifecycle controls, event display, JSON inspection, and bounded
logs. Xvfb was used for headless UI/screenshot smoke testing.

What this proved:

- The UI can be built and exercised without a physical display.
- The UI can display a logical four-guest topology and host-side status.
- A launcher can safely control the reviewed outer QEMU process model.

What it did not prove:

- The buttons did not independently control four production Linux guests.
- The logical roles were bundled into a bring-up image rather than being
  complete independent services.
- The UI did not become a secure arbitrary guest shell or QEMU monitor.
- A screenshot only proves rendering; it is not VM or network evidence.

The launcher was deliberately made fail-closed and does not accept arbitrary
shell text, arbitrary script paths, QEMU monitor commands, or host-directory
mounts. That is safer, but it also means the requested general-purpose
"send any shell command to any VM" feature was not implemented.

### 3. Vibe Kanban staging

The pinned `vibe-kanban@0.1.44` package was run in a temporary, rootless
development container with no credentials, a read-only root, dropped
capabilities, no project mount, and only `127.0.0.1:3030` published. A health
request returned HTTP 200. The helper scripts were made reproducible and the
control-plane health probe was restricted to that fixed local check.

This was useful staging evidence, but it was not the requested deployment:

- the process ran in a host/container staging environment, not inside the
  seL4 guest;
- host-loopback access to the staging container is not proof of guest ingress;
- the guest image was launched with `-nic none`;
- the actual seL4 Ethernet driver, firewall, and host forwarding path were
  never completed;
- no authenticated Kanban-to-Codex production broker was proven.

The initial `npx` command was rejected by the tool's high-risk review. Later
user authorization allowed the pinned, temporary, credential-free staging
run. That authorization did not make the staged service a production guest.

### 4. seL4/CAmkES four-guest bring-up

The V2 tree was reduced to four logical guests using one common
`rootfs-v2.cpio`: Kanban, Codex, Git, and Build. The generated CapDL/CAmkES
artifacts declared separate guest resources and a Codex-centered connector
topology. The role status and small job-control records were moved toward
bounded JSONL over upstream CAmkES dataports and notifications.

The KVM integration probe demonstrated, for the reviewed bring-up image:

- four guest Linux startup milestones;
- role tags and connector setup;
- bounded JSONL role/job handoffs;
- Codex-to-Build source/result activity for the tested bundle;
- a Git status request and acknowledgement round trip;
- an offline Build check/test/run workflow;
- the embedded/staged Kanban service responding in-guest.

These results are meaningful bring-up evidence, but not production service
evidence. The image still used retained compatibility plumbing for the larger
Build path, and the following were absent: real host ingress, real Git push,
real secret provisioning, final network policy, and reset/reprovision proof
for the production workflow.

### 5. Network implementation

The image intentionally used `-nic none`. This kept the incomplete network
path from being mistaken for a secure one. A QEMU `hostfwd` option alone could
not solve the problem because there was no guest network device or reviewed
seL4 firewall path behind it.

The missing production path requires an x86 Ethernet driver composition such
as the upstream `Ethdriver82574`/`HWEthDriver82574` path, a QEMU-compatible
device, correct PCI/MMIO/IRQ declarations, a firewall component, and a fresh
KVM test proving host loopback access while the other guests retain their
declared restrictions.

What was not completed:

- no `Ethdriver82574` composition was integrated into the Vibe guest;
- no firewall allowlist was proven;
- no Codex egress policy was enforced in the guest;
- no Build post-provisioning network shutdown was proven;
- no end-to-end host `127.0.0.1 -> guest Kanban -> response` test exists.

Do not globally add `-nic user`, a shared virtual switch, or unrestricted
QEMU forwarding and call that the final security policy. It would violate the
role-specific network design.

### 6. Debian/Ubuntu root filesystem attempt

There was an attempt to build a slim Debian root filesystem with debootstrap.
Running the second stage directly on the host led to errors such as:

```text
cat: /debootstrap/mirror: No such file or directory
cannot create //test-dev-null: Permission denied
umount: //test-dev-null: ... Device or resource busy
```

This happened because debootstrap's second stage expects a correctly prepared
rootfs, metadata, mounts, and root privileges. It is not equivalent to a
Python virtual environment. `chroot` changes the apparent root directory and
lets package-install scripts operate as if they are inside the target system;
it also needs privileged filesystem operations.

The safer development direction was to run the rootfs preparation inside the
authorized Podman build environment or use the existing Buildroot/upstream
guest artifact rather than improvising a host chroot. The old Linux 4.8 image
seen during earlier VMM work was an upstream/tutorial-era artifact; it was not
the agreed final slim Debian/Ubuntu image and should not be treated as a
requirement.

### 7. CapDL allocator and memory-probe failures

The x86 dynamic CapDL loader produced errors resembling:

```text
Untyped Retype: Insufficient memory
```

The message was confusing because the allocator could exhaust one normal
untyped region and then continue probing another. The x86/no-DTS target did
not have the same convenient static allocator input path as the ARM examples.
The attempted mitigation combined a larger outer QEMU memory map with a
deterministic build-time preflight that approximated loader-owned object-size
and alignment calculations.

This reduced the observed bring-up failure and allowed a reviewed KVM run,
but it is not a formal CapDL allocator fix. It does not prove that arbitrary
guest sizes, future kernel objects, or a different Microkit/libvmm revision
will fit. A future session must inspect the exact generated CapDL object graph,
untyped sizes, alignment, reservation policy, and allocator source before
claiming this issue is solved.

### 8. Arbitrary shell/command execution request

The user requested a host port carrying JSON commands that identify a VM and
run shell commands or scripts in it. A bounded local control plane was added,
but it intentionally rejects arbitrary shell text, arbitrary URLs, arbitrary
script paths, and host-directory access.

Why the requested unrestricted version was not accepted:

- it would turn a host socket into a privileged remote shell;
- it would bypass the role topology and broker policy;
- a prompt-injected Codex process could use it to execute commands in other
  guests or on the host unless every command were separately capability-gated;
- raw command output would reintroduce the untrusted-output problem.

The next design should expose named operations such as `install_toolchain`,
`send_bundle`, `build`, `test`, `run`, `git_status`, and `reset`, each with
fixed schemas and quotas. It should not expose a general shell until a
separate security decision explicitly accepts that risk.

### 9. Credentials and Git

The intended design moved the Git SSH key to the Git guest and kept it out of
Codex and Build. The image and tested buffers did not contain the real key,
which is a positive isolation result.

However, no real key provisioning or authenticated Git push was completed.
Linux users and file permissions can reduce accidental reads, but they are not
a complete boundary against a compromised Codex process with execution access
to a Git client. A Git helper can hide the key from ordinary filesystem reads,
but it must still authorize the Git operation and prevent unauthorized
destinations, branches, force pushes, and repository changes.

The next session should use a dedicated, least-privilege Git identity and
test a narrowly scoped push or pull-request flow only after the transport and
audit path are stable. Never place the real key in the common image, Build
guest, shared raw buffer, logs, or crash artifacts.

### 10. Multi-vCPU investigation

Microkit itself has host multi-core support merged and documented as available
since 2.1.0. That does not mean a Linux guest automatically receives several
virtual CPUs.

The relevant VMM finding was:

- libvmm main supports multiple guest vCPUs on ARM, documented up to 16;
- the current libvmm manual explicitly says x86 multiple-vCPU support is in
  progress;
- the x86 example still models one VMM TCB and one vCPU;
- an `x86_multiple_vcpu_hacking` branch exists, but its May 12, 2026 WIP tip
  says the approach was effectively abandoned for further thought;
- the merged multiple-vCPU work was the ARM line, not a completed x86 path.

Therefore, the Cargo Build guest cannot currently be assumed to have multiple
x86 vCPUs under upstream Microkit/libvmm. This was one of the reasons the
original expectation that a fast multi-core Cargo VM would work immediately
was not met. ARM emulation is possible in principle but is not a practical
replacement for native x86 KVM performance, and it would require a compatible
ARM Microkit/libvmm deployment rather than merely changing a QEMU flag.

## What was actually validated

The strongest validated results were:

- host-side Rust tests and policy checks;
- native egui rendering and Xvfb screenshot smoke testing;
- launcher/QEMU PID-safe lifecycle handling;
- a four-role CAmkES/CapDL bring-up image;
- separate declared guest resource pools and role tags;
- bounded JSONL status/control records;
- a tested Codex/Build bundle workflow for the reviewed fixture;
- a Git status/acknowledgement fixture, not a real push;
- an in-guest Kanban health response in the bring-up environment;
- a host/container Vibe staging health response on `127.0.0.1:3030`;
- no-network QEMU launch and serial-log evidence.

These are component and bring-up results. They do not add up to a completed
secure AI environment. In particular, a passing integration log is not proof
of production network policy, real credentials, real external services, or a
formal security argument.

## What must not be repeated

1. Do not start by adding more UI, schemas, policy modules, or defensive
   abstractions. First make one real guest service run and communicate through
   one real, minimal connector path.
2. Do not treat host/container Vibe staging as guest Vibe Kanban.
3. Do not treat a green CapDL or KVM bring-up log as proof of production
   security.
4. Do not use the old Linux 4.8 artifact as the target OS merely because an
   upstream example used it.
5. Do not use `hostfwd` without a functioning guest NIC and firewall.
6. Do not enable unrestricted networking globally to make a demo work.
7. Do not put real API or Git credentials into an image while the transport,
   reset, logging, and guest boundaries are still experimental.
8. Do not claim x86 multi-vCPU support based on ARM commits or a WIP branch.
9. Do not create separate image-build pipelines for every role unless the
   security model proves that they are necessary. The common-image approach is
   simpler and was the intended V2 direction.
10. Do not add a general arbitrary-shell control socket as a shortcut.
11. Do not spend time rewriting seL4, CAmkES-generated code, or the VMM in
   Rust. Keep low-level upstream interfaces in their native language and keep
   the authored policy/transport adapter small and reviewed.

## Suggested clean-session starting point

The next session should begin with a read-only audit of the current tree and
then choose one narrow acceptance target:

```text
1. Verify the current V2 files, image hashes, Microkit/libvmm revisions, and
   the exact host/QEMU architecture.
2. Decide whether x86 one-vCPU guests are acceptable. If not, stop and choose
   an upstream-supported ARM multi-vCPU target or a different hypervisor.
3. Run exactly one seL4 Linux guest with the common image.
4. Make the real Kanban service boot inside that guest.
5. Implement only the reviewed Vibe NIC/firewall/host-loopback path.
6. Prove host health access in a fresh KVM log.
7. Add one bounded Codex-centered connector and one named file-transfer
   operation.
8. Only then add Git provisioning and the offline Build workflow.
```

The most useful existing references are:

- `v2/ALCATRAZVM_CONVERSATION_CONTEXT.md` — longer historical context;
- `v2/V2_IMPLEMENTATION_STATUS.md` — implementation checkpoint;
- `v2/V2_COMPLETION_AUDIT.md` — evidence-based requirement audit;
- `v2/HOST_NETWORK_IMPLEMENTATION.md` — exact missing network path;
- `ALCATRAZVM_V2_ARCHITECTURE.md` — V2 architecture;
- `v2/SECURE_AI_VM_ARCHITECTURE.md` — measured implementation snapshot;
- `v2/README.md` — commands and current build/run gates.

## Final status

The result is a useful but incomplete research prototype and bring-up
scaffold. The implementation is not worthless, but it is not the finished
system that was repeatedly implied during the earlier session. The next
attempt should be smaller, evidence-driven, and organized around one real
end-to-end guest workflow instead of accumulating host-side abstractions.
