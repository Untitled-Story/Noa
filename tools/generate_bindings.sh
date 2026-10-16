#!/bin/sh
set -eu

# Normal builds use the checked-in Rust bindings; regeneration needs flatc.
noa_flatc=${1:-flatc}
noa_version=$("$noa_flatc" --version)
if [ "$noa_version" != "flatc version 25.12.19" ]; then
    echo "Expected flatc version 25.12.19, got: $noa_version" >&2
    exit 1
fi
noa_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$noa_root"
"$noa_flatc" --rust --gen-object-api --rust-serialize -o crates/noa-format/src/generated schemas/mesh.fbs schemas/noir.fbs
python3 tools/derive_native_serde.py
rustfmt --edition 2024 crates/noa-format/src/generated/*_generated.rs

# Optional output directory for consumers' TypeScript bindings.
if [ "$#" -ge 2 ]; then
    "$noa_flatc" --ts --gen-object-api -o "$2" schemas/noir.fbs schemas/mesh.fbs
fi
