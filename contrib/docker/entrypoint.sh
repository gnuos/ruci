#!/bin/sh
# Rucid CI entrypoint script

set -e

# Create necessary directories if they don't exist
mkdir -p /var/lib/ruci/jobs /var/lib/ruci/run /var/lib/ruci/archive /var/lib/ruci/logs /run/rucid

# Set ownership
chown -R ruci:ruci /var/lib/ruci /var/log/ruci /run/rucid 2>/dev/null || true

# Execute the command
exec "$@"
