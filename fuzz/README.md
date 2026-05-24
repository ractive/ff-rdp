# ff-rdp fuzz harnesses

`cargo-fuzz` targets covering parsers that consume attacker-controlled
input:

| target                   | parser entry point                                 |
|--------------------------|----------------------------------------------------|
| `transport_recv_from`    | `ff_rdp_core::transport::recv_from`                |
| `parse_page_map_str`     | `ff_rdp_cli::page_map::parse_page_map_str`         |
| `parse_script_file`      | `ff_rdp_cli::script::format::parse_script_str`     |

This is a standalone crate (its own `[workspace]`), kept out of the main
workspace so stable `cargo test --workspace` does not try to build the
nightly-only `libfuzzer-sys` dependency.

## Platform requirements

The fuzz harnesses require the **`x86_64-unknown-linux-gnu`** target (not
`musl`).  libFuzzer's address-sanitiser is incompatible with musl libc, which
causes a link failure on Ubuntu runners that default to `x86_64-unknown-linux-musl`.
The CI `fuzz` job pins `targets: x86_64-unknown-linux-gnu` in the
`dtolnay/rust-toolchain` step for this reason.  Run locally on any Linux/macOS
host with the standard nightly toolchain.

## Install

```sh
rustup install nightly
cargo install cargo-fuzz
```

## Run locally

```sh
# 60 s per target — matches CI. Seeds in `seeds/<target>/` are copied into
# the runtime corpus by cargo-fuzz on first run.
cargo +nightly fuzz run transport_recv_from seeds/transport_recv_from -- -max_total_time=60
cargo +nightly fuzz run parse_page_map_str  seeds/parse_page_map_str  -- -max_total_time=60
cargo +nightly fuzz run parse_script_file   seeds/parse_script_file   -- -max_total_time=60
```

Seeds are checked into `seeds/` (small valid examples). The runtime corpus
and crash artifacts live in `corpus/` and `artifacts/` and are
`.gitignore`d.

## When CI finds a crash

1. The CI job uploads the minimised input as an artifact.
2. Reproduce locally: `cargo +nightly fuzz run <target> <path-to-input>`.
3. Open an issue with the minimised input attached.
4. Fix the parser, check the input into `fuzz/seeds/<target>/` as a
   regression seed (the runtime `corpus/` is `.gitignore`d, so only
   `seeds/` is version-controlled — see `CONTRIBUTING.md`).

See `CONTRIBUTING.md` for the full policy.
