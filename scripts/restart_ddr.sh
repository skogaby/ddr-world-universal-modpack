#!/bin/bash
# Restart DDR World on the remote Windows machine
HOST=$(cat /tmp/sshhost)
USER=$(cat /tmp/sshuser)
PASS=$(cat /tmp/sshpass)
SSH="sshpass -p $PASS ssh -o StrictHostKeyChecking=no $USER@$HOST"

$SSH "taskkill /f /im spice64.exe" 2>/dev/null
sleep 2
$SSH 'schtasks /run /tn "StartDDR"' 2>&1

echo "Waiting for DDR to start..."
for i in $(seq 1 30); do
  sleep 2
  if $SSH "tasklist | findstr spice64" 2>/dev/null | grep -q spice64; then
    echo "DDR is running (took ~$((i*2))s)"
    exit 0
  fi
done
echo "DDR failed to start within 60s"
exit 1
