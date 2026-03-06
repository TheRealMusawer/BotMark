 BotMark v0.1.1 — Persistent AFK & Remote Control


A high-performance, low-level Rust Minecraft bot built for 24/7 AFK farming on Render.
Includes a real-time web dashboard with WASD controls and remote command execution.
🌟 Features
24/7 Render Hosting: Built-in tiny_http server to satisfy Render’s health checks and prevent service sleeping.
Advanced Anti-AFK: Uses Cubic Easing (
) for "jitter" movement, making bot activity look human to anti-cheats.
Remote Control Dashboard: Modern dark-mode UI with:
WASD Buttons: Move the bot manually from your browser.
Command Bar: Execute slash commands or send chat messages remotely.
AFK Toggle: Enable or disable jitter movement instantly.
Cracked Server Support: Skips Mojang authentication for instant connection to offline-mode servers.
Low-Level Efficiency: Built on the Pumpkin protocol for minimal RAM and CPU usage.
🚀 How to Deploy (Render)
Environment Variables: In your Render Web Service settings, add the following:
MC_SERVER: The IP and Port (e.g., play.server.com:25565).
MC_USER: Your desired bot username.
PORT: 10000
Build Settings:
Build Command: cargo build --release
Start Command: ./target/release/botmark
🧠 Code Breakdown: How it Works
1. The Async Heart (main.rs)
lookup_host: Automatically resolves domain names to IPs.
tokio::select!: Manages three simultaneous tasks:
Web Dashboard: Listens for your clicks and commands.
Packet Reader: Decodes incoming server data (KeepAlives, Spawns).
Bot Ticker: Runs the movement and rotation logic every 50ms.
2. The Protocol Logic (client.rs)
ConnectionState: A state machine that moves from Handshake ➔ Login ➔ Config ➔ Play.
send_packet: Manually serializes Rust structs into raw Minecraft hex data.
process_packets:
KeepAlive: Automatically sends a response to the server to prevent "Timed Out" kicks.
CPlayerPosition: Syncs the bot's coordinates when the server teleports it.
3. The Movement Engine (tick_movement)
Atomic State: Uses crossbeam atomics so the Web Dashboard and the Bot Loop can share coordinates safely without lagging.
Cubic Easing: Instead of teleporting, the bot calculates a smooth curve between two points, simulating a player slightly shifting their weight or moving back and forth.
Rotation & Swing: Periodically sends SPlayerRotation and SSwingArm packets to ensure the server sees constant, non-static activity.
4. The Web Dashboard (start_web_dashboard)
Action Handling: When you click "W", the dashboard sends an HTTP request to the bot. The bot intercepts this, calculates the new Z coordinate, and pushes a SPlayerPosition packet to Minecraft.
Command Injection: Takes your text input, URI-decodes it, and wraps it into a SChatCommand packet with a valid 1.21.1 BitSet acknowledgment.
🛠️ Dependencies
pumpkin-protocol: Low-level Minecraft packet handling.
tokio: The async runtime for handling networking and web traffic.
tiny_http: A lightweight, synchronous web server for the dashboard.
crossbeam: Ultra-fast atomic variables for thread-safe positioning.
