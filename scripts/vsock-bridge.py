#!/usr/bin/env python3
"""Narrow host bridge for the Vibe guest -> Codex guest acceptance test."""
import selectors, socket, sys, time

def recv_line(sock):
    data = b''
    while not data.endswith(b'\n'):
        chunk = sock.recv(64)
        if not chunk: raise RuntimeError('vsock endpoint closed during handshake')
        data += chunk
    return data

if len(sys.argv) != 3:
    raise SystemExit('usage: vsock-bridge.py CODEX_UDS VIBE_UDS')
codex_path, vibe_path = sys.argv[1:]
vibe_listener_path = vibe_path + '_7001'
try:
    import os
    os.unlink(vibe_listener_path)
except FileNotFoundError: pass
listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
listener.bind(vibe_listener_path); listener.listen(1); listener.settimeout(10)
codex = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM); codex.settimeout(10)
for _ in range(100):
    try: codex.connect(codex_path); break
    except OSError: time.sleep(.1)
else: raise RuntimeError('codex Firecracker UDS did not open')
codex.sendall(b'CONNECT 7000\n')
ack = recv_line(codex)
if not ack.startswith(b'OK '): raise RuntimeError(f'codex handshake rejected: {ack!r}')
vibe, _ = listener.accept(); vibe.settimeout(10)
codex.settimeout(None); vibe.settimeout(None)
sel = selectors.DefaultSelector(); sel.register(codex, selectors.EVENT_READ, vibe); sel.register(vibe, selectors.EVENT_READ, codex)
while sel.get_map():
    for key, _ in sel.select(10):
        source, target = key.fileobj, key.data
        data = source.recv(4096)
        if not data:
            sel.unregister(source); source.close(); target.close(); sel.unregister(target); break
        target.sendall(data)
listener.close(); os.unlink(vibe_listener_path)
