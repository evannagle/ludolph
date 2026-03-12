---
name: health-check
description: Check Ludolph/Lu health on Pi and Mac, diagnose issues
---

# /health-check - Ludolph Health Diagnostics

Quickly diagnose Ludolph/Lu connectivity and service health.

## Checks Performed

1. **Pi connectivity** - Can we reach the Pi via SSH?
2. **Service status** - Is `ludolph.service` running?
3. **Port binding** - Is port 8202 bound correctly?
4. **API health** - Does `/health` endpoint respond?
5. **Recent logs** - Any errors in the last 20 lines?
6. **MCP tools** - Are lu_send/lu_history available?

## Commands

Run these in sequence:

```bash
# 1. Check Pi reachability
ssh pi "echo 'Pi reachable'"

# 2. Check service status
ssh pi "systemctl --user status ludolph --no-pager"

# 3. Check port binding
ssh pi "lsof -i :8202 | head -5"

# 4. Check API health
ssh pi "curl -s http://localhost:8202/health"

# 5. Check recent logs
ssh pi "journalctl --user -u ludolph -n 20 --no-pager"
```

## Common Issues & Fixes

### Service not running
```bash
ssh pi "systemctl --user start ludolph"
```

### Port conflict (Address already in use)
```bash
ssh pi "pkill -9 -f '.ludolph/bin/lu'; sleep 2; systemctl --user start ludolph"
```

### Old binary deployed
```bash
ssh pi "systemctl --user stop ludolph && cp ~/ludolph/target/release/lu ~/.ludolph/bin/lu && systemctl --user start ludolph"
```

### Need to rebuild on Pi
```bash
ssh pi "cd ~/ludolph && git fetch origin develop && git checkout origin/develop && source ~/.cargo/env && cargo build --release"
```

### MCP not configured
Check `.mcp.json` in project root points to correct Python file and has valid auth token.

## Quick Health Summary

After running checks, summarize:
- Service: running/stopped/error
- Port: bound/conflict/free
- API: responding/timeout/error
- Logs: clean/warnings/errors

## Recovery Workflow

If unhealthy:
1. Stop service: `ssh pi "systemctl --user stop ludolph"`
2. Deploy latest: `ssh pi "cp ~/ludolph/target/release/lu ~/.ludolph/bin/lu"`
3. Start service: `ssh pi "systemctl --user start ludolph"`
4. Verify: `ssh pi "curl -s http://localhost:8202/health"`
