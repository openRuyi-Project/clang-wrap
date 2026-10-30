#!/bin/bash
set -eu

script=$1
SOURCES_BASE_DIR=$2
# Load only the helper, without running package builds or accessing the network.
source <(sed -n '/^prepare_tarball_source() {$/,/^}$/p' "$script")
mkdir -p "$SOURCES_BASE_DIR/libyang/1.0/libyang-1.0"
mkdir -p "$SOURCES_BASE_DIR/libyang/2.0/libyang-2.0"
for version in 1.0 2.0 1.0; do
    actual=$(prepare_tarball_source libyang "$version" unused unused)
    expected="$SOURCES_BASE_DIR/libyang/$version/libyang-$version"
    if [ "$actual" != "$expected" ]; then
        echo "Requested $version, expected $expected, got $actual" >&2
        exit 1
    fi
done
