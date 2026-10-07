#!/bin/sh
# Cargo hashes this checkout-specific path to isolate workspace artifacts, not registry dependencies.
exec "$@"
