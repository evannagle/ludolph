# Getting Started with Plugins

Plugins extend Lu with new capabilities — email, calendar, billing, whatever you need.
They run as separate MCP servers alongside Lu's core vault tools.

## Finding Plugins

```bash
lu plugin search email
lu plugin search calendar
```

This searches the [community registry](https://github.com/ludolph-community/plugin-registry).

## Installing

```bash
lu plugin install lu-email
```

Or from a local path during development:

```bash
lu plugin install /path/to/my-plugin
```

## Setting Up Credentials

Most plugins need API keys or OAuth tokens:

```bash
lu plugin setup lu-email
```

Follow the prompts. Credentials are stored locally in `~/.ludolph/plugins/`.

## Checking Status

```bash
lu plugin list              # Show installed plugins
lu plugin check lu-email    # Health check a specific plugin
lu plugin logs lu-email     # View recent logs
```

## Scheduled Tasks

Some plugins run automatically on a schedule — morning email digests, meeting note imports, daily syncs. Results appear as vault notes you can review whenever.

To see what's scheduled:

```bash
lu plugin list --schedules
```

You'll get a Telegram notification when scheduled tasks complete (if `notify = true` in the plugin config).

## Updating

```bash
lu plugin update lu-email   # Update one
lu plugin update --all      # Update all
```

## Removing

```bash
lu plugin remove lu-email
```

## Troubleshooting

| Problem | Fix |
|---------|-----|
| "Plugin not found" | Check spelling, or install from path |
| "Credential not configured" | Run `lu plugin setup <name>` |
| Tools not appearing | Run `lu plugin check <name>` |
