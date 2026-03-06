# ==========================================================================================
#  PROJECT: BOTMARK v0.1.1 [INTERNAL SYSTEM ARCHITECTURE]
#  REVISION: 2026.03.06.01
#  AUTHOR: MARK-BOT-CORE
#  CLASSIFICATION: PERSISTENT CLOUD-LINKED MINECRAFT BOT
#  TARGET: tuff.ws [MC 1.21.1 / PROTOCOL 767]
# ==========================================================================================

[core.metadata]
project_name        = "BotMark"
version             = "0.1.1"
edition             = "2024 (Bleeding Edge Rust)"
compiler_target     = "x86_64-unknown-linux-gnu"
optimization_level  = "3 (LTO + Strip-Symbols)"
binary_size_est     = "2.4MB (Static Build)"

[system.architecture_diagram]
# L0: Physical Layer -> TCP/IP Stream
# L1: Transport Layer -> Tokio Async Net + lookup_host DNS
# L2: Protocol Layer -> Pumpkin Java Decoder/Encoder
# L3: State Layer -> AtomicCell ConnectionState Machine
# L4: Logic Layer -> Anti-AFK Jitter / Rotation / Swing
# L5: Control Layer -> tiny_http Web Dashboard Interface

[dependency.inventory]
# Networking & Runtime
tokio               = { version = "1.49.0", features = ["net", "io-util", "rt-multi-thread", "macros", "signal", "time"] }
tiny_http           = { version = "0.12", desc = "Health-check & Dashboard server" }

# Minecraft Protocol (Low-Level)
pumpkin-protocol    = { git = "https://github.com", features = ["serverbound", "clientbound"] }
pumpkin-util        = { git = "https://github.com" }
pumpkin-data        = { git = "https://github.com" }

# Concurrency & Utility
crossbeam           = { version = "0.8.4", desc = "Lock-free atomic sharing for Dashboard vs Bot Loop" }
uuid                = { version = "1.20.0", desc = "Bot identity generation" }
rand                = { version = "0.8.5", desc = "Randomized jitter intervals" }
urlencoding         = { version = "2.1.3", desc = "Dashboard command string decoding" }
simple_logger       = { version = "5.1.0", desc = "Standard output formatting" }

[logic.main_rs_breakdown]
lines_01_20         = "Imports: std::net, Arc, Duration, Ordering, tokio::net, tiny_http."
lines_21_40         = "Struct BotConfig: Stores boolean flags for rotation and swing."
lines_41_100        = "fn start_web_dashboard: spawns blocking thread for HTTP server."
lines_101_110       = "Dashboard Action Mapping: Matches /action?type=W/A/S/D to send_chat_or_cmd."
lines_111_120       = "AFK Toggle: Reads current AtomicBool and flips state via ordering."
lines_121_130       = "Command Injection: Captures URI-encoded msg= string, decodes, and spawns tokio task."
lines_131_200       = "HTML/CSS Template: Inline dark-mode CSS with D-Pad grid layout and JS fetch API."
lines_201_250       = "fn main: Entry point. Pulls MC_SERVER and MC_USER from std::env."
lines_251_260       = "DNS Resolution: lookup_host converts domain strings to SocketAddr."
lines_261_280       = "TCP Connection: establishes stream with 10s timeout via tokio::select!."
lines_281_300       = "Task Spawning: reader_task (Packets) and ticker_task (Logic) initialized via Arc<Client>."

[logic.client_rs_breakdown]
lines_01_30         = "Protocol Structures: Import MinecraftVersion, ClientPacket, ConnectionState."
lines_31_60         = "Struct Client: Defines AtomicCell fields for X, Y, Z, Yaw, Pitch, and Progress."
lines_61_80         = "impl Client::new: Splits TcpStream into OwnedReadHalf and OwnedWriteHalf."
lines_81_100        = "fn send_packet: Generic T: ClientPacket implementation. Handles packet ID VarInt serialization."
lines_101_130       = "fn send_chat_or_cmd: Logic gate for slash commands vs standard chat messages."
lines_131_140       = "SPlayerPosition Mapping: Updates internal atomic coordinates and pushes to TCP buffer."
lines_141_160       = "fn join_server: Sequence -> SHandShake(State 2) -> SLoginStart(Offline UUID)."
lines_161_200       = "fn process_packets: The core loop. Handles CLoginSuccess -> CFinishConfig -> CKeepAlive."
lines_201_220       = "Anti-Timeout: SKeepAlive(0) auto-reply logic to bypass AFK-kick timers."
lines_221_250       = "fn tick_config: Master loop calling movement, swing, and rotation sub-functions."
lines_251_280       = "fn tick_movement: Implements Cubic Easing (3t² - 2t³) for jitter fluidity."
lines_281_300       = "fn tick_rotation: Random Yaw adjustment to simulate 'looking around'."

[network.protocol_767_specs]
version             = "1.21.1"
connection_flow     = "Handshake -> Login -> Config -> Play"
auth_mode           = "Offline (Cracked)"
encryption          = "DISABLED (Omit EncryptionRequest handler)"
compression         = "DISABLED (Server-dependent, defaults to off on tuff.ws)"
packet_id_chat      = "0x06 (Serverbound)"
packet_id_cmd       = "0x04 (Serverbound)"
bitset_format       = "3-byte bitfield (Required for 1.21.1 acknowledgments)"

[anti_afk.vibration_profile]
mode                = "Cubic-Jitter"
x_jitter            = "0.0 Blocks (Static)"
y_jitter            = "0.0 Blocks (Static)"
z_jitter            = "0.5 Blocks (Dynamic Easing)"
swing_rate          = "Every 2 seconds (VarInt 0 = Main Hand)"
rotation_range      = "10 degrees per jitter"
tick_rate           = "20 TPS (50ms intervals)"

[deployment.render_paas]
tier                = "Free Tier (Web Service)"
port_binding        = "10000 (Internal) -> 443 (External HTTPS)"
idle_timeout        = "15 Minutes (Requires external ping)"
build_pipeline      = "Cargo Fetch -> Cargo Build (Release) -> Binary Strip"
container_base      = "Debian Bullseye / Rust Stable"

[ui.ux_elements]
color_bg            = "#0f0f0f (Jet Black)"
color_accent        = "#00ff41 (Matrix Green)"
color_alert         = "#ff0000 (Warning Red)"
layout_type         = "Grid / Flexbox Hybrid"
mobile_friendly     = "true (Viewport meta-tag enabled)"
api_calls           = "Vanila JavaScript Fetch (Async)"

[security.operational_notes]
cracked_safety      = "offline-mode bots are vulnerable to username-spoofing; use unique names."
render_exposure     = "Dashboard is public; obfuscate the URL or use a long random app name."
tuff_ws_notes       = "Server uses standard plugins; jitter is sufficient to bypass AFK-kicker."

[maintenance.troubleshooting]
error_timeout       = "Check MC_SERVER env var format (ip:port)."
error_render_kill   = "Ensure PORT is set to 10000 and dashboard is binding to 0.0.0.0."
error_disconnect    = "Server might have a CAPTCHA; use the Dashboard /cmd bar to solve."

# ==========================================================================================
#  DOCUMENT END - [LINES 1-500] - MANIFEST SEALED
# ==========================================================================================
