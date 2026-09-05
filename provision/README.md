# Provisioning contract

Role provisioning is offline-safe by default. A production image builder must
replace the role body with digest-pinned installation and write `manifest.json`
before creating `sealed.offline`. No script accepts URLs, shell text, keys, or
host mounts from a guest request.
