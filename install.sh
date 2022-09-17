#!/usr/bin/env bash

set -ex

plugin_dir=$HOME/.config/obs-studio/plugins/livesplit/bin/64bit
mkdir -p $plugin_dir
cargo build --release
ln -fs $(pwd)/target/release/liblivesplit.so $plugin_dir/liblivesplit.so
