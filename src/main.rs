use std::{net::SocketAddr, sync::Arc, time::Duration, sync::atomic::Ordering};
use client::Client;
use tokio::{net::{TcpStream, lookup_host}, time::timeout};
use tiny_http::{Server, Response, Header};

mod client;

// We'll use this struct internally to pass settings to the ticker
pub struct BotConfig {
    pub enable_rotation: bool,
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
                            let current = b.afk_active_val();
                            b.set_afk(!current);
                        },
                        _ => {}
                    }
                });
            }

            if url.starts_with("/cmd?msg=") {
                let msg = url.split("msg=").last().unwrap_or("");
                let decoded = urlencoding::decode(msg).unwrap_or_default().into_owned();
                if !decoded.is_empty() {
                    let b = bot.clone();
                    tokio::spawn(async move { b.send_chat_or_cmd(&decoded).await; });
                }
            }

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
                <p style="font-size: 10px; color: #444; margin-top: 20px;">CONTROL PANEL ACTIVE</p>
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

    // Pull from Environment Variables (Set these in Render!)
    let server_raw = std::env::var("MC_SERVER").unwrap_or_else(|_| "127.0.0.1:25565".to_string());
    let username = std::env::var("MC_USER").unwrap_or_else(|_| "BotMark".to_string());
    
    // Resolve DNS (converts mc.server.com to IP)
    log::info!("Resolving {}...", server_raw);
    let address = lookup_host(&server_raw).await
        .expect("Failed to resolve server address")
        .next()
        .expect("No IP address found for host");

    log::info!("Connecting to {} as {}...", address, username);
    
    let stream = match timeout(Duration::from_secs(10), TcpStream::connect(address)).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => { log::error!("Connection error: {}", e); return; }
        Err(_) => { log::error!("Connection timed out"); return; }
    };

    let client = Arc::new(Client::new(stream));
    let config = Arc::new(BotConfig { enable_rotation: true, enable_swing: true });
    
    start_web_dashboard(client.clone()).await;
    client.join_server(address, username).await;

    let reader_bot = client.clone();
    let reader_task = tokio::spawn(async move {
        loop { if !reader_bot.process_packets().await { break; } }
    });

    let ticker_bot = client.clone();
    let ticker_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(50));
        loop {
            interval.tick().await;
            ticker_bot.tick_config(&config).await;
        }
    });

    tokio::select! {
        _ = tokio::signal::ctrl_c() => log::info!("Shutdown."),
        _ = reader_task => log::error!("Network failure."),
    }
}
