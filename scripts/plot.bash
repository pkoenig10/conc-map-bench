#!/usr/bin/env bash

set -e

BIN=./target/release/conc-map-bench
DATA_DIR=results

cargo build --release

function plot {
    echo "$DATA_DIR/$1.csv"
    cat "$DATA_DIR/$1.csv" | "$BIN" plot "$DATA_DIR" "$1"
}

plot ReadHeavy.std
# plot Exchange.std
# plot RapidGrow.std
# plot ReadHeavy.ahash
# plot Exchange.ahash
# plot RapidGrow.ahash
