use crossbeam::atomic::AtomicCell;
use pumpkin_data::packet::CURRENT_MC_PROTOCOL;
use pumpkin_protocol::codec::var_int::VarInt;
use pumpkin_protocol::java::client::config::CFinishConfig;
use pumpkin_protocol::java::client::login::CLoginSuccess;
use pumpkin_protocol::java::client::play::{CKeepAlive, CPlayerPosition};
use pumpkin_protocol::java::packet_decoder::TCPNetworkDecoder;
use pumpkin_protocol::java::packet_encoder::TCPNetworkEncoder;
use pumpkin_protocol::java::server::config::SAcknowledgeFinishConfig;
use pumpkin_protocol::java::server::handshake::SHandShake;
use pumpkin_protocol::java::server::login::SLoginStart;
use pumpkin_protocol::java::server::play::{SKeepAlive, SPlayerPosition, SPlayerRotation, SSwingArm, SChatCommand, SChatMessage};
use pumpkin_protocol::ser::NetworkWriteExt;
use pumpkin_protocol::{ClientPacket, ConnectionState, MinecraftVersion};
use pumpkin_util::math::vector3::Vector3;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
use tokio::io::{BufReader, BufWriter};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::Args;

pub struct Client {
    pub connection_state: AtomicCell<ConnectionState>,
    pub closed: AtomicBool,
    pub network_writer: Arc<Mutex<TCPNetworkEncoder<BufWriter<OwnedWriteHalf>>>>,
    pub network_reader: Arc<Mutex<TCPNetworkDecoder<BufReader<OwnedReadHalf>>>>,
    entity_id: AtomicI32,
    is_loaded: AtomicBool,
    swing_cooldown: AtomicU32,
    afk_active: AtomicBool,
    // Position
    current_x: AtomicCell<f64>, current_y: AtomicCell<f64>, current_z: AtomicCell<f64>,
    start_z: AtomicCell<f64>, target_z: AtomicCell<f64>,
    move_progress: AtomicCell<f32>, move_cooldown: AtomicU32,
    // Rotation
    current_yaw: AtomicCell<f32>, current_pitch: AtomicCell<f32>,
    start_yaw: AtomicCell<f32>, target_yaw: AtomicCell<f32>,
    rotation_progress: AtomicCell<f32>, rotation_cooldown: AtomicU32,
}

impl Client {
    pub fn new(stream: TcpStream) -> Self {
        let (connection_reader, connection_writer) = stream.into_split();
        Self {
            connection_state: AtomicCell::new(ConnectionState::HandShake),
            network_writer: Arc::new(Mutex::new(TCPNetworkEncoder::new(BufWriter::new(connection_writer)))),
            network_reader: Arc::new(Mutex::new(TCPNetworkDecoder::new(BufReader::new(connection_reader)))),
            entity_id: AtomicI32::new(0),
            closed: AtomicBool::new(false),
            swing_cooldown: AtomicU32::new(0),
            is_loaded: AtomicBool::new(false),
            afk_active: AtomicBool::new(true),
            rotation_cooldown: AtomicU32::new(0),
            rotation_progress: AtomicCell::new(1.0),
            current_yaw: AtomicCell::new(0.0), current_pitch: AtomicCell::new(0.0),
            start_yaw: AtomicCell::new(0.0), target_yaw: AtomicCell::new(0.0),
            current_x: AtomicCell::new(0.0), current_y: AtomicCell::new(0.0), current_z: AtomicCell::new(0.0),
            start_z: AtomicCell::new(0.0), target_z: AtomicCell::new(0.0),
            move_progress: AtomicCell::new(1.0),
            move_cooldown: AtomicU32::new(0),
        }
    }

    pub fn set_afk(&self, active: bool) {
        self.afk_active.store(active, Ordering::SeqCst);
        if active { self.move_cooldown.store(0, Ordering::Relaxed); }
        log::info!("AFK Mode toggled to: {}", active);
    }

    pub async fn send_packet<P: ClientPacket>(&self, packet: &P) {
        let mut buf = Vec::new();
        let mut writer = &mut buf;
        let _ = writer.write_var_int(&VarInt(P::PACKET_ID.latest_id));
        packet.write_packet_data(writer, &MinecraftVersion::V_1_21_1).unwrap();
        let _ = self.network_writer.lock().await.write_packet(buf.into()).await;
    }

    /// Dashboard logic: Sends a command or chat message
    pub async fn send_chat_or_cmd(&self, input: &str) {
        if input.starts_with('/') {
            let cmd = input.strip_prefix('/').unwrap_or(input);
            self.send_packet(&SChatCommand {
                command: cmd.to_string(),
                timestamp: 0,
                salt: 0,
                argument_signatures: Vec::new(),
                message_count: VarInt(0),
                acknowledgments: vec![0u8; 3].into(), // Fixed-size bitset for 1.21.1
            }).await;
            log::info!("Executed Dashboard Command: /{}", cmd);
        } else {
            self.send_packet(&SChatMessage {
                message: input.to_string(),
                timestamp: 0,
                salt: 0,
                signature: None,
                message_count: VarInt(0),
                acknowledgments: vec![0u8; 3].into(),
            }).await;
            log::info!("Sent Dashboard Chat: {}", input);
        }
    }

    pub async fn join_server(&self, addr: SocketAddr, username: String) {
        self.send_packet(&SHandShake {
            protocol_version: VarInt(CURRENT_MC_PROTOCOL as i32),
            server_address: addr.ip().to_string(),
            server_port: addr.port(),
            next_state: VarInt(2), // Login
        }).await;
        self.connection_state.store(ConnectionState::Login);

        self.send_packet(&SLoginStart {
            name: username,
            uuid: Uuid::new_v4(),
        }).await;
    }

    pub async fn process_packets(&self) -> bool {
        let mut reader = self.network_reader.lock().await;
        match reader.read_packet().await {
            Ok(Some(raw)) => {
                let id = raw.id.0;
                match self.connection_state.load() {
                    ConnectionState::Login => {
                        if id == CLoginSuccess::PACKET_ID.latest_id {
                            log::info!("Login Success! Moving to Config state.");
                            self.connection_state.store(ConnectionState::Config);
                        }
                    }
                    ConnectionState::Config => {
                        if id == CFinishConfig::PACKET_ID.latest_id {
                            log::info!("Config Finished! Moving to Play state.");
                            self.send_packet(&SAcknowledgeFinishConfig {}).await;
                            self.connection_state.store(ConnectionState::Play);
                        }
                    }
                    ConnectionState::Play => {
                        if id == CKeepAlive::PACKET_ID.latest_id {
                            // Respond with 0 to keep connection alive
                            self.send_packet(&SKeepAlive { keep_alive_id: 0 }).await;
                        } else if id == CPlayerPosition::PACKET_ID.latest_id {
                            if !self.is_loaded.load(Ordering::Relaxed) {
                                self.is_loaded.store(true, Ordering::Relaxed);
                                log::info!("Bot spawned into world.");
                            }
                        }
                    }
                    _ => {}
                }
                true
            }
            Ok(None) => false,
            Err(_) => false,
        }
    }

    pub async fn tick(&self, args: &Args) {
        if self.connection_state.load() != ConnectionState::Play || !self.is_loaded.load(Ordering::Relaxed) {
            return;
        }
        if args.enable_rotation { self.tick_rotation().await; }
        if args.enable_swing { self.tick_swing().await; }
        if self.afk_active.load(Ordering::Relaxed) { self.tick_movement().await; }
    }

    async fn tick_movement(&self) {
        let progress = self.move_progress.load();
        if progress >= 1.0 {
            if self.move_cooldown.fetch_sub(1, Ordering::Relaxed) <= 1 {
                let cz = self.current_z.load();
                self.start_z.store(cz);
                let target = if (self.target_z.load() - cz).abs() < 0.1 { cz + 0.5 } else { cz - 0.5 };
                self.target_z.store(target);
                self.move_progress.store(0.0);
                self.move_cooldown.store(100, Ordering::Relaxed);
            }
        } else {
            let np = (progress + 0.1).min(1.0);
            self.move_progress.store(np);
            let t = 3.0 * np.powi(2) - 2.0 * np.powi(3);
            let nz = self.start_z.load() + (self.target_z.load() - self.start_z.load()) * t as f64;
            self.current_z.store(nz);
            self.send_packet(&SPlayerPosition {
                position: Vector3::new(self.current_x.load(), self.current_y.load(), nz),
                collision: 1, 
            }).await;
        }
    }

    async fn tick_swing(&self) {
        if self.swing_cooldown.fetch_sub(1, Ordering::Relaxed) <= 1 {
            self.send_packet(&SSwingArm { hand: VarInt(0) }).await;
            self.swing_cooldown.store(40, Ordering::Relaxed);
        }
    }

    async fn tick_rotation(&self) {
        let progress = self.rotation_progress.load();
        if progress >= 1.0 {
            if self.rotation_cooldown.fetch_sub(1, Ordering::Relaxed) <= 1 {
                self.start_yaw.store(self.current_yaw.load());
                // Randomly rotate within a small range to simulate activity
                self.target_yaw.store(self.current_yaw.load() + 10.0);
                self.rotation_progress.store(0.0);
                self.rotation_cooldown.store(200, Ordering::Relaxed);
            }
        } else {
            let np = (progress + 0.05).min(1.0);
            self.rotation_progress.store(np);
            let nyaw = self.start_yaw.load() + (self.target_yaw.load() - self.start_yaw.load()) * np;
            self.current_yaw.store(nyaw);
            self.send_packet(&SPlayerRotation {
                yaw: nyaw,
                pitch: self.current_pitch.load(),
                collision: 1,
            }).await;
        }
    }
}
