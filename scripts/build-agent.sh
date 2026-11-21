#!/usr/bin/env bash
# Cross-compile the embedded remote agent (static linux-x86_64 musl binary).
# On macOS this uses zig as the linker/cc; on Linux install `musl-tools` and
# it uses musl-gcc directly.
set -euo pipefail
cd "$(dirname "$0")/.."

target=x86_64-unknown-linux-musl
rustup target add "$target"

if [ "$(uname)" = "Darwin" ]; then
    command -v zig >/dev/null || { echo "need zig: brew install zig"; exit 1; }
    work="$(mktemp -d)"
    cat > "$work/zigcc" <<'EOF'
#!/usr/bin/env bash
out=()
for a in "$@"; do
  case "$a" in --target=*) ;; *) out+=("$a");; esac
done
exec zig cc -target x86_64-linux-musl "${out[@]}"
EOF
    printf '#!/bin/sh\nexec zig ar "$@"\n' > "$work/zigar"
    chmod +x "$work/zigcc" "$work/zigar"
    export PATH="$work:$PATH"
    export CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=zigcc
    export CC_x86_64_unknown_linux_musl=zigcc
    export AR_x86_64_unknown_linux_musl=zigar
    export RUSTFLAGS="-C link-self-contained=no"
else
    command -v musl-gcc >/dev/null || { echo "need musl-gcc: apt install musl-tools"; exit 1; }
    export CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=musl-gcc
    export CC_x86_64_unknown_linux_musl=musl-gcc
fi

cargo build --release --target "$target"
cp "target/$target/release/devin-usage" assets/agent-linux-x86_64
ls -lh assets/agent-linux-x86_64
