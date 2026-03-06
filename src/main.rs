use std::{net::SocketAddr, sync::Arc, time::Duration};
use clap::Parser;
use tokio::{net::TcpStream, time::timeout};
mod client;
use client::Client;

#[derive(Parser, Debug)]
pub struct Args {
    #[arg(long)] pub ip: String,
    #[arg(short, long, default_value_t = 1)] pub count: u32,
    #[arg(short, long, default_value_t = 5000)] pub timeout: u64,
    #[arg(long, default_value_t = true)] pub enable_rotation: bool,
    #[arg(long, default_value_t = true)] pub enable_swing: bool,
}

async fn start_web_dashboard(bot: Arc<Client>) {
    let port = std::env::var("PORT").unwrap_or_else(|_| "10000".to_string());
    let server = tiny_http::Server::http(format!("0.0.0.0:{}", port)).unwrap();
    log::info!("Dashboard active on port {}", port);
    
    tokio::task::spawn_blocking(move || {
        for request in server.incoming_requests() {
            let response_text = match request.url() {
                "/move" => { bot.set_afk(true); "Movement: ENABLED" },
                "/stop" => { bot.set_afk(false); "Movement: DISABLED" },
                _ => "Bot Dashboard. /move | /stop",
            };
            let _ = request.respond(tiny_http::Response::from_string(response_text));
        }
    });
}

#[tokio::main]
async fn main() {
    simple_logger::init_with_level(log::Level::Info).unwrap();
    let args = Arc::new(Args::parse());
    let address = args.ip.parse::<SocketAddr>().expect("Invalid IP:Port");

    let stream = timeout(Duration::from_millis(args.timeout), TcpStream::connect(address))
        .await.expect("Connect Timeout").expect("Connect Failed");

    let client = Arc::new(Client::new(stream));
    start_web_dashboard(client.clone()).await;

    client.join_server(address, "HelperBot".to_string()).await;

    let bot = client.clone();
    let cloned_args = args.clone();
    tokio::spawn(async move {
        let mut tick_interval = tokio::time::interval(Duration::from_millis(50));
        loop {
            tokio::select! {
                res = bot.process_packets() => { if !res { break; } }
                _ = tick_interval.tick() => { bot.tick(&cloned_args).await; }
            }
        }
    });

    tokio::signal::ctrl_c().await.unwrap();
}
