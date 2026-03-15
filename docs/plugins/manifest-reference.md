# Plugin Manifest Reference

The `lu-plugin.toml` file defines your plugin's metadata, tools, credentials, and schedules.

## Required Fields

```toml
[plugin]
name = "my-plugin"              # Lowercase, hyphens only
version = "0.1.0"               # Semantic version
description = "What it does"    # One line
```

## Optional Metadata

```toml
[plugin]
author = "your-name"
license = "MIT"
repository = "https://github.com/you/my-plugin"
```

## Runtime

How Lu runs your plugin:

```toml
[plugin.runtime]
type = "external"       # Plugin runs as separate process
package = "my-plugin"   # Package name
runtime = "uvx"         # "uvx" (Python) or "npx" (Node)
```

## Credentials

Secrets your plugin needs. Lu prompts for these during `lu plugin setup`.

```toml
[plugin.credentials]
API_KEY = { description = "Service API key", required = true }
WEBHOOK_URL = { description = "Optional webhook", required = false }
```

Access in Python via environment variables:

```python
import os
api_key = os.environ.get("API_KEY")
```

## Tools

Declare tools for discoverability (implementation lives in server.py):

```toml
[[plugin.tools]]
name = "summarize_inbox"
description = "Summarize unread emails"
vault_output = true
output_path = "Inbox/email/{date}.md"

[[plugin.tools]]
name = "send_reply"
description = "Send an email reply"
vault_output = false
```

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Tool identifier |
| `description` | string | What it does |
| `vault_output` | bool | Creates vault notes? |
| `output_path` | string | Where notes land |

**Output path variables:** `{date}` (YYYY-MM-DD), `{datetime}` (ISO format), `{name}` (from tool arguments)

## Schedules

Run tools automatically:

```toml
[[plugin.schedules]]
name = "morning_digest"
cron = "0 8 * * *"          # 8am daily
tool = "summarize_inbox"
notify = true               # Telegram notification when done

[[plugin.schedules]]
name = "hourly_sync"
cron = "0 * * * *"          # Every hour
tool = "sync_tasks"
notify = false
```

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Schedule identifier |
| `cron` | string | Cron expression |
| `tool` | string | Tool to run |
| `notify` | bool | Send Telegram notification |

## Full Example

```toml
[plugin]
name = "lu-email"
version = "1.0.0"
description = "Gmail integration with vault-first email management"
author = "ludolph-community"
license = "MIT"
repository = "https://github.com/ludolph-community/lu-email"

[plugin.runtime]
type = "external"
package = "lu-email"
runtime = "uvx"

[plugin.credentials]
GOOGLE_CLIENT_ID = { description = "OAuth client ID", required = true }
GOOGLE_CLIENT_SECRET = { description = "OAuth client secret", required = true }

[[plugin.tools]]
name = "email_summarize_inbox"
description = "Summarize unread emails and create vault note"
vault_output = true
output_path = "Inbox/email-summaries/{date}.md"

[[plugin.schedules]]
name = "inbox_digest"
cron = "0 8 * * *"
tool = "email_summarize_inbox"
notify = true
```
