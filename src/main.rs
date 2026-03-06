use std::{net::SocketAddr, sync::Arc, time::Duration, sync::atomic::Ordering};
use clap::Parser;
use client::Client;
use tokio::{net::TcpStream, time::timeout};
use tiny_http::{Server, Response, Header};

mod client;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
    #[arg(long)]
    pub ip: String,
    #[arg(long, default_value = "BotMark")]
    pub username: String,
    #[arg(short, long, default_value_t = 5000)]
    pub timeout: u64,
    #[arg(long, default_value_t = true)]
    pub enable_rotation: bool,
    #[arg(long, default_value_t = true)]
    pub enable_swing: bool,
}

async fn start_web_dashboard(bot: Arc<Client>) {
    let port = std::env::var("PORT").unwrap_or_else(|_| "10000".to_string());
    let addr = format!("0.0.0.0:{}", port);
    let server = Server::http(&addr).expect("Failed to bind to port");
    log::info!("Dashboard active on http://{}", addr);
    
    tokio::task::spawn_blocking(move || {
        for request in server.incoming_requests() {
            let url = request.url();
            
            // 1. Handle WASD & Toggles
            if url.starts_with("/action") {
                let action = url.split("type=").last().unwrap_or("");
                let b = bot.clone();
                tokio::spawn(async move {
                    match action {
                        "W" => b.send_chat_or_cmd("/move forward").await,
                        "A" => b.send_chat_or_cmd("/move left").await,
                        "S" => b.send_chat_or_cmd("/move back").await,
                        "D" => b.send_chat_or_cmd("/move right").await,
                        "toggle_afk" => {
                            let current = b.afk_active_val(); // Helper to read atomic
                            b.set_afk(!current);
                        },
                        _ => {}
                    }
                });
            }

            // 2. Handle Command Bar
            if url.starts_with("/cmd?msg=") {
                let msg = url.split("msg=").last().unwrap_or("");
                let decoded = urlencoding::decode(msg).unwrap_or_default().into_owned();
                if !decoded.is_empty() {
                    let b = bot.clone();
                    tokio::spawn(async move { b.send_chat_or_cmd(&decoded).await; });
                }
            }

            // 3. Serve UI
            let html = r#"
            <!DOCTYPE html>
            <html>
            <head>
                <meta name="viewport" content="width=device-width, initial-scale=1">
                <title>BotMark Control</title>
                <style>
                    body { background: #0f0f0f; color: #00ff41; font-family: 'Segoe UI', Tahoma, sans-serif; display: flex; flex-direction: column; align-items: center; padding: 20px; }
                    .card { background: #1a1a1a; padding: 20px; border-radius: 12px; border: 1px solid #333; box-shadow: 0 8px 32px rgba(0,0,0,0.5); width: 350px; text-align: center; }
                    .dpad { display: grid; grid-template-columns: repeat(3, 1fr); gap: 10px; margin: 20px auto; width: 180px; }
                    button { background: #2a2a2a; color: #00ff41; border: 1px solid #00ff41; padding: 15px; cursor: pointer; border-radius: 8px; font-weight: bold; transition: 0.2s; }
                    button:hover { background: #00ff41; color: #000; box-shadow: 0 0 15px #00ff41; }
                    button:active { transform: scale(0.95); }
                    .afk-toggle { grid-column: span 3; background: #300; border-color: #f00; color: #f00; margin-top: 10px; }
                    .afk-toggle:hover { background: #f00; color: #000; box-shadow: 0 0 15px #f00; }
                    .input-group { margin-top: 25px; display: flex; gap: 5px; }
                    input { flex: 1; background: #000; border: 1px solid #333; color: #fff; padding: 10px; border-radius: 5px; outline: none; }
                    input:focus { border-color: #00ff41; }
                    h2 { margin: 0 0 10px 0; font-size: 1.2rem; letter-spacing: 2px; }
                </style>
            </head>
            <body>
                <div class="card">
                    <h2>BOTMARK v0.1.1</h2>
                    <div class="dpad">
                        <div></div><button onclick="fetch('/action?type=W')">W</button><div></div>
                        <button onclick="fetch('/action?type=A')">A</button>
                        <button onclick="fetch('/action?type=S')">S</button>
                        <button onclick="fetch('/action?type=D')">D</button>
                        <button class="afk-toggle" onclick="fetch('/action?type=toggle_afk')">TOGGLE ANTI-AFK</button>
                    </div>
                    <div class="input-group">
                        <input type="text" id="m" placeholder="Command or chat...">
                        <button onclick="const i=document.getElementById('m'); fetch('/cmd?msg='+encodeURIComponent(i.value)); i.value='';">SEND</button>
                    </div>
                </div>
                <p style="font-size: 10px; color: #444; margin-top: 20px;">CONNECTED TO MC SERVER via RENDER</p>
            </body>
            </html>
            "#;

            let response = Response::from_string(html)
                .with_header(Header::from_bytes(&b"Content-Type"[..], &b"text/html"[..]).unwrap());
            let _ = request.respond(response);
        }
    });
}

#[tokio::main]
async fn main() {
    simple_logger::init_with_level(log::Level::Info).unwrap();
    let args = Arc::new(Args::parse());
    let address = args.ip.parse::<SocketAddr>().expect("Invalid IP:Port");

    log::info!("Establishing connection to {}...", address);
    
    let stream = match timeout(Duration::from_millis(args.timeout), TcpStream::connect(address)).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => { log::error!("Connection error: {}", e); return; }
        Err(_) => { log::error!("Connection timed out"); return; }
    };

    let client = Arc::new(Client::new(stream));
    
    // Start UI first so Render detects the service is "Live"
    start_web_dashboard(client.clone()).await;

    // Join Server (Cracked/Offline mode supported)
    client.join_server(address, args.username.clone()).await;

    // Packet Processing Task
    let reader_bot = client.clone();
    let reader_task = tokio::spawn(async move {
        loop {
            if !reader_bot.process_packets().await { break; }
        }
    });

    // Bot Tick Task (Movement/Anti-AFK)
    let ticker_bot = client.clone();
    let ticker_args = args.clone();
    let ticker_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(50));
        loop {
            interval.tick().await;
            ticker_bot.tick(&ticker_args).await;
        }
    });

    tokio::select! {
        _ = tokio::signal::ctrl_c() => log::info!("Shutdown initiated."),
        _ = reader_task => log::error!("Network failure."),
    }
}
