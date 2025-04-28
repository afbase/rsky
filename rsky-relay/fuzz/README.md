# Fuzz

This folder is for fuzzing the merkle search tree inversion code.  I borrow some of the example keys from the atproto [interop tests](https://github.com/bluesky-social/atproto-interop-tests/) and this tweaks the keys to generate a variety of test cases.

## Getting Started

1. Make sure you have cargo-fuzz installed: `cargo install cargo-fuzz`
1. Install the nightly rust toolchain: `rustup toolchain install nightly`

## Running Fuzz Tests

1. Navigate to the fuzz directory: `cd /path/to/rsky/rsky-relay/fuzz`
1. Run the fuzz tests with: `cargo +nightly fuzz run mst_example_keys`

There is a crash that happens fairly quickly.  I haven't looked into it just quite yet.
