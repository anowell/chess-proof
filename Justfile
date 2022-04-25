run:
    cargo build --release
    time target/release/chess-proof

flamegraph:
    CARGO_PROFILE_RELEASE_DEBUG=true PERF=/usr/lib/linux-tools/5.4.0-109-generic/perf cargo flamegraph
