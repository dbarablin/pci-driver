#!/bin/bash
# ---------------------------------------------------------------------------- #

set -o errexit -o pipefail -o nounset

if (( $# > 1 )); then
    >&2 echo "Usage: $0 [<toolchain>]"
    exit 2
elif (( $# == 1 )); then
    rustup=( rustup run -- "$1" )
else
    rustup=()
fi

function __log_and_run() {
    printf '\033[0;33m%s\033[0m\n' "$*"
    "$@"
}

function __cargo() {
    __log_and_run "${rustup[@]}" cargo "$@"
}

export CARGO_TERM_COLOR=always

script_dir="$( dirname "$0" )"
if [[ "${script_dir}" != . ]]; then
    __log_and_run cd "${script_dir}"
fi

# ---------------------------------------------------------------------------- #

function __rust_version_is_at_least() {
    (echo "min $1"; "${rustup[@]}" rustc --version) |
        sort -Vk2 | tail -1 | grep -q rustc
}

if [[ -v PCI_DRIVER_FEATURES ]]; then
    features=( --no-default-features --features="${PCI_DRIVER_FEATURES}" )
elif __rust_version_is_at_least 1.52; then
    # feature "_unsafe-op-in-unsafe-fn" only works with Rust 1.52+
    features=( --all-features )
else
    features=()
fi

__cargo fmt --all -- --check

__cargo clippy --all-targets "${features[@]}" -- --deny warnings

# this catches problems in doc comments
__cargo doc

# run doc tests with default features
__cargo test --doc

# run other tests with requested features
__cargo test --all-targets "${features[@]}"

# ---------------------------------------------------------------------------- #
