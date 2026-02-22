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

Tailscale creates a secure mesh network between your devices. Access your Pi from anywhere - coffee shop, airport, different WiFi networks.

#### Install Tailscale

**On Mac:**
```bash
brew install --cask tailscale
```
Then open Tailscale from Applications and sign in.

**On Pi:**
```bash
curl -fsSL https://tailscale.com/install.sh | sh
sudo tailscale up
```

Follow the URL to authorize the Pi in your Tailscale admin console.

#### Enable Tailscale SSH (Recommended)

Tailscale can handle SSH authentication directly - no need for separate SSH keys:

1. Go to [Tailscale Admin Console](https://login.tailscale.com/admin/acls)
2. Enable "SSH" in Access Controls, or add to your ACL:
   ```json
   "ssh": [
     {"action": "accept", "src": ["*"], "dst": ["*"], "users": ["autogroup:nonroot"]}
   ]
   ```
3. On the Pi, enable Tailscale SSH:
   ```bash
   sudo tailscale set --ssh
   ```

Now you can SSH using your Tailscale identity - no password or key needed.

#### Find Your Pi's Tailscale Address

```bash
# On the Pi
tailscale status
```

You'll see output like:
```
100.100.100.100  raspberrypi    linux   -
```

Use the hostname shown (e.g., `raspberrypi`) with your tailnet domain. Find your tailnet name at [Tailscale Admin > DNS](https://login.tailscale.com/admin/dns).

**Common formats:**
- `raspberrypi` (if MagicDNS enabled)
- `raspberrypi.tail1234.ts.net` (full Tailscale hostname)
- `100.100.100.100` (Tailscale IP - always works)

#### Test Connection

```bash
# From Mac, with Tailscale running
ssh pi@raspberrypi
```

If using Tailscale SSH, you'll connect without password prompts.

#### Use in lu setup

When prompted for "Pi hostname or IP", enter your Tailscale hostname:
```
π Pi hostname or IP
  : raspberrypi
```

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

**Tailscale: "command not found"**
- Tailscale isn't installed or not in PATH
- On Mac: `brew install --cask tailscale`
- On Pi: `curl -fsSL https://tailscale.com/install.sh | sh`

**Tailscale: Can't reach Pi**
- Check both devices show "connected" in `tailscale status`
- Verify Pi is authorized in [Tailscale Admin](https://login.tailscale.com/admin/machines)
- Try the Tailscale IP directly (e.g., `ssh pi@100.x.x.x`)
