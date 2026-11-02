#!/usr/bin/env bash
# Deploy with progress on the dashboard. See deploy.py.
exec python3 "$(dirname "$0")/deploy.py" "$@"
