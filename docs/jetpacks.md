# Jetpacks

Jetpacks are recipes for getting more out of Ludolph. Each one combines a few of Lu's tools — schedules, learn, observations, vault search — into something that actually helps you, rather than just demonstrating that AI can do things.

Pick what sounds useful. Skip what doesn't.

## Morning Brief

The original jetpack. Lu reads your email, Slack, calendar, vault tasks, and optionally news, then sends you a digest every morning via Telegram.

### What you need

- Google Workspace MCP (for Gmail + Calendar)
- Slack MCP (if you use Slack)
- Vault with tasks/journals (Obsidian)
- A schedule

### Setup

1. Enable the MCPs you need:

```
/mcp enable google-workspace
/mcp enable slack
```

2. Configure credentials when prompted (OAuth for Google, bot token for Slack).

3. Tell Lu to create the schedule:

```
Lu, create a morning brief schedule that runs at 7:00 AM every weekday.
Pull from my email (unread, last 12 hours), calendar (today's events),
Slack (unread mentions), and vault tasks (due today or overdue).
Format it as a quick digest — not a novel.
```

Lu will create a schedule with the right cron expression and a prompt that hits all those sources.

### What it looks like

```
Good morning.

Calendar (3 events)
  9:00  Standup
  11:00 Design review with Sarah
  2:00  Dentist

Email (5 unread)
  - RE: Q2 budget approval (CFO, urgent)
  - Invoice from AWS
  - 3 newsletters

Slack (2 mentions)
  - #engineering: Dave asked about the API migration timeline
  - #random: someone posted a cat

Vault (2 tasks due)
  - Finish book proposal draft (due today)
  - Call dentist about Elvis's appointment

Have a good one.
```

### Tuning

- Too noisy? Tell Lu: "Skip newsletters in the morning brief."
  Lu saves this as a preference observation and applies it next time.
- Want news? Tell Lu: "Add top 3 headlines from Hacker News to my morning brief."
  Lu will add an RSS feed check to the schedule prompt.
- Wrong timezone? Tell Lu your timezone once. It remembers.

## Weekly Review

A Friday afternoon summary of what happened in your vault, what got done, and what's still open.

### Setup

```
Lu, create a weekly review that runs Friday at 4 PM.
Summarize: files I created or modified this week, tasks completed,
tasks still open, and any patterns you notice.
```

### What Lu does

- Searches vault for files modified in the last 7 days
- Checks completed vs open tasks
- Notes any recurring themes or projects getting attention
- Sends the digest via Telegram

## Meeting Prep

Before a meeting, Lu pulls your calendar event details and finds related notes in your vault.

### How to use

This one isn't scheduled — just ask before a meeting:

```
Lu, prep me for my 11:00 meeting.
```

Lu will:
1. Check your calendar for the 11:00 event (attendees, agenda, location)
2. Search your vault for notes related to the topic or attendees
3. Surface any recent observations about the project
4. Give you a quick briefing

## Learn a Codebase

When you join a new project or want Lu to understand a repo you're working on:

```
Lu, learn github:company/big-project
```

Lu shallow-clones the repo, indexes the code and docs, and can then answer questions about it:

```
Lu, how does authentication work in big-project?
Lu, teach me the API layer --for coders
```

### Forgetting

When you're done with a codebase:

```
Lu, forget github:company/big-project
```

Clean removal. No residue.

## Research Assistant

When you're researching a topic and want Lu to read and remember sources:

```
Lu, learn https://docs.example.com/api-reference
Lu, learn ~/Downloads/whitepaper.pdf
Lu, learn ~/Research/papers/
```

Then ask questions:

```
Lu, what does the whitepaper say about rate limiting?
Lu, teach me the API authentication model --for coders
```

Lu searches across all learned content and your vault simultaneously.

## Inbox Zero Assist

A daily triage of your email, categorized by action needed:

### Setup

```
Lu, every morning at 7:30, check my unread email and categorize each one:
- Action required (I need to respond or do something)
- FYI (informational, no action needed)
- Skip (newsletters, automated notifications)
Just show me the Action Required ones in full. Summarize the FYI list.
Skip everything else.
```

## Building Your Own

A jetpack is just a combination of:

1. **A schedule** (when to run)
2. **A prompt** (what to do)
3. **MCPs** (where to get data)
4. **Observations** (how to tune it over time)
5. **Knowledge** (what Lu has learned, optional)

The pattern is always the same:
- Tell Lu what you want, in plain language
- Lu creates the schedule and prompt
- Try it for a few days
- Tell Lu what to change ("less verbose", "skip newsletters", "add weather")
- Lu saves your preferences as observations and adjusts

No config files. No YAML. Just conversation.

### Supercharging with Learn

If your jetpack needs domain knowledge — say your morning brief should reference internal docs, or your meeting prep should know about the project codebase — teach Lu first:

```
Lu, learn github:company/big-project
Lu, learn https://internal-docs.company.com/handbook
```

Now when your jetpack runs, Lu searches that learned content alongside your vault. The research assistant and codebase learning jetpacks above work this way.

### How observations work

When you tell Lu "skip newsletters," Lu saves an observation:

```
[pref] Morning brief: skip newsletters in email summary
```

Next time the morning brief runs, that observation is in Lu's system prompt. Lu applies it without you having to say it again. Observations accumulate — each correction makes the jetpack better.

To see what Lu remembers: "What observations do you have about me?"

For the full picture on how Lu's memory works, see [Memory System](MEMORY.md). For the learn/teach pipeline, see [Learning and Teaching](learn.md).
