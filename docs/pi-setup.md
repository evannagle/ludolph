# Raspberry Pi Setup

This guide helps you set up SSH access from your Mac to your Raspberry Pi.

## Prerequisites

- Raspberry Pi with Raspberry Pi OS installed
- Pi connected to your network (Ethernet or WiFi)
- SSH enabled on the Pi

## 1. Find Your Pi

If your Pi is on the same local network:

```bash
# Pi usually advertises itself as pi.local
ping pi.local
```

If that doesn't work, check your router's admin page for connected devices, or run:

```bash
# Scan local network for Raspberry Pis
arp -a | grep -i "b8:27:eb\|dc:a6:32\|e4:5f:01"
```

## 2. Set Up SSH Key Authentication

Ludolph requires key-based SSH auth (no password prompts).

```bash
# Generate a key if you don't have one
ssh-keygen -t ed25519

# Copy your key to the Pi
ssh-copy-id pi@pi.local
```

Test it works without a password:

```bash
ssh pi@pi.local "echo success"
```

## 3. Hostname Options

When `lu setup` asks for "Pi hostname or IP", you can use:

| Option | Example | When to use |
|--------|---------|-------------|
| Local hostname | `pi.local` | Same network, home use |
| Local IP | `192.168.1.50` | Same network, if .local doesn't resolve |
| Tailscale | `pi.tailnet.ts.net` | Remote access from anywhere |

### Local Network Only

Use `pi.local` or the IP address. Works when Mac and Pi are on the same WiFi/Ethernet.

### Remote Access with Tailscale

For access from anywhere:

1. Install Tailscale on both Mac and Pi:
   ```bash
   # On Mac
   brew install tailscale

   # On Pi
   curl -fsSL https://tailscale.com/install.sh | sh
   ```

2. Log in on both devices:
   ```bash
   sudo tailscale up
   ```

3. Find your Pi's Tailscale hostname:
   ```bash
   tailscale status
   ```

4. Use that hostname (e.g., `pi.tailnet.ts.net`) in `lu setup`

## 4. Verify Connection

After setup, test with:

```bash
lu pi
```

Should show:

```
Pi Connection

  [•??] Connecting to pi@pi.local...
  [•ok] Connected to pi@pi.local
```

## Troubleshooting

**"Connection refused"**
- SSH isn't enabled on Pi. Run `sudo raspi-config` → Interface Options → SSH → Enable

**"Permission denied"**
- Key auth not set up. Run `ssh-copy-id pi@pi.local`

**"Host not found"**
- Pi isn't reachable. Check it's powered on and connected to network
- Try IP address instead of hostname

**Timeout**
- Pi is on different network. Consider Tailscale for remote access
