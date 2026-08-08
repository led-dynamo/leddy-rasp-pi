#![forbid(unsafe_code)]

use futures_util::{SinkExt, StreamExt};
use leddy_interfaces::{
    DeviceCapabilities, DeviceCommand, DeviceEvent, DevicePlatform, DeviceTelemetry,
    DeviceTransport, DisplayConfig, MessageEnvelope, PixelOrigin, RECOMMENDED_MAX_HEIGHT,
    RECOMMENDED_MAX_WIDTH, RECOMMENDED_MIN_HEIGHT, RECOMMENDED_MIN_WIDTH,
};
use leddy_lib::{FrameBuffer, render_message_frame};
use std::{
    env, fs, io,
    path::{Path, PathBuf},
    time::Instant,
};
use tokio::time::{Duration, MissedTickBehavior, interval, sleep};
use tokio_tungstenite::{connect_async, tungstenite::Message};

const MIN_RECONNECT_BACKOFF: Duration = Duration::from_millis(250);
const MAX_RECONNECT_BACKOFF: Duration = Duration::from_secs(30);
const HEALTHY_SESSION: Duration = Duration::from_secs(10);

#[derive(Debug)]
struct RuntimeSettings {
    device_id: String,
    ws_url: String,
    config_path: PathBuf,
    snapshot_path: Option<PathBuf>,
    frame_interval: Duration,
    telemetry_interval: Duration,
}

impl RuntimeSettings {
    fn from_env() -> Self {
        Self {
            device_id: env::var("LEDDY_DEVICE_ID").unwrap_or_else(|_| "pi-development".into()),
            ws_url: env::var("LEDDY_DEVICE_WS_URL")
                .unwrap_or_else(|_| "ws://localhost:8080/v1/ws/devices".into()),
            config_path: env::var_os("LEDDY_CONFIG_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/var/lib/leddy/display-config.json")),
            snapshot_path: env::var_os("LEDDY_FRAME_SNAPSHOT").map(PathBuf::from),
            frame_interval: Duration::from_millis(env_u64("LEDDY_FRAME_INTERVAL_MS", 50).max(1)),
            telemetry_interval: Duration::from_secs(
                env_u64("LEDDY_TELEMETRY_INTERVAL_SECS", 5).max(1),
            ),
        }
    }
}

#[derive(Debug)]
struct ActiveMessage {
    message: MessageEnvelope,
    started: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FrameStats {
    pixels: usize,
    lit_pixels: usize,
}

#[derive(Debug)]
struct FrameSink {
    snapshot_path: Option<PathBuf>,
}

impl FrameSink {
    fn new(snapshot_path: Option<PathBuf>) -> Self {
        Self { snapshot_path }
    }

    fn present(&self, config: &DisplayConfig, frame: &FrameBuffer) -> io::Result<FrameStats> {
        let pixels = encode_device_frame(config, frame);
        let stats = FrameStats {
            pixels: pixels.len(),
            lit_pixels: pixels.iter().filter(|pixel| **pixel != 0).count(),
        };
        self.write_snapshot(&pixels)?;
        Ok(stats)
    }

    fn clear(&self, config: &DisplayConfig) -> io::Result<()> {
        self.write_snapshot(&vec![0; config.pixel_count()])
    }

    fn write_snapshot(&self, pixels: &[u8]) -> io::Result<()> {
        let Some(path) = &self.snapshot_path else {
            return Ok(());
        };
        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, pixels)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let settings = RuntimeSettings::from_env();
    let fallback = config_from_env()?;
    let mut config = load_config(&settings.config_path, fallback)?;
    validate_supported_config(&config)?;
    let mut sink = FrameSink::new(settings.snapshot_path.clone());
    let mut backoff = MIN_RECONNECT_BACKOFF;

    loop {
        let session_started = Instant::now();
        match run_session(&settings, &mut config, &mut sink).await {
            Ok(()) => tracing::warn!("device socket closed"),
            Err(error) => tracing::error!(%error, "device session failed"),
        }

        backoff = if session_started.elapsed() >= HEALTHY_SESSION {
            MIN_RECONNECT_BACKOFF
        } else {
            next_backoff(backoff)
        };
        tracing::info!(delay_ms = backoff.as_millis(), "reconnecting to Leddy API");
        sleep(backoff).await;
    }
}

async fn run_session(
    settings: &RuntimeSettings,
    config: &mut DisplayConfig,
    sink: &mut FrameSink,
) -> Result<(), Box<dyn std::error::Error>> {
    let (socket, _) = connect_async(&settings.ws_url).await?;
    let (mut writer, mut reader) = socket.split();
    let hello = DeviceEvent::Hello {
        device_id: settings.device_id.clone(),
        firmware_version: env!("CARGO_PKG_VERSION").into(),
        capabilities: DeviceCapabilities {
            max_width: RECOMMENDED_MAX_WIDTH,
            max_height: RECOMMENDED_MAX_HEIGHT,
            color_depth_bits: 8,
            supports_brightness: true,
            platform: Some(DevicePlatform::RaspberryPi),
            transports: vec![DeviceTransport::Wifi, DeviceTransport::Ethernet],
        },
    };
    send_event(&mut writer, hello).await?;

    let mut active: Option<ActiveMessage> = None;
    let mut frame_tick = interval(settings.frame_interval);
    frame_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut telemetry_tick = interval(settings.telemetry_interval);
    telemetry_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            incoming = reader.next() => {
                let Some(incoming) = incoming else {
                    break;
                };
                match incoming? {
                    Message::Text(text) => {
                        let command: DeviceCommand = serde_json::from_str(&text)?;
                        match command {
                            DeviceCommand::Show(message) => {
                                message.validate()?;
                                let command_id = message.id.clone();
                                active = Some(ActiveMessage { message, started: Instant::now() });
                                send_event(&mut writer, DeviceEvent::Ack { command_id }).await?;
                            }
                            DeviceCommand::Clear => {
                                active = None;
                                sink.clear(config)?;
                                send_event(
                                    &mut writer,
                                    DeviceEvent::Ack { command_id: "clear".into() },
                                ).await?;
                            }
                            DeviceCommand::Configure(next) => {
                                validate_supported_config(&next)?;
                                persist_config(&settings.config_path, &next)?;
                                *config = next;
                                if let Some(active) = active.as_mut() {
                                    active.started = Instant::now();
                                }
                                sink.clear(config)?;
                                send_event(
                                    &mut writer,
                                    DeviceEvent::Ack { command_id: "configure".into() },
                                ).await?;
                            }
                            DeviceCommand::Ping { nonce } => {
                                send_event(&mut writer, DeviceEvent::Pong { nonce }).await?;
                            }
                        }
                    }
                    Message::Ping(payload) => {
                        writer.send(Message::Pong(payload)).await?;
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
            _ = frame_tick.tick() => {
                let rendered = if let Some(active) = active.as_ref() {
                    let elapsed_ms = active.started.elapsed().as_millis() as u64;
                    Some(render_message_frame(config, &active.message, elapsed_ms)?)
                } else {
                    None
                };

                match rendered {
                    Some(Some(frame)) => {
                        let stats = sink.present(config, &frame)?;
                        tracing::debug!(
                            pixels = stats.pixels,
                            lit_pixels = stats.lit_pixels,
                            brightness = config.brightness,
                            "presented LED frame"
                        );
                    }
                    Some(None) => {
                        active = None;
                        sink.clear(config)?;
                    }
                    None => {}
                }
            }
            _ = telemetry_tick.tick() => {
                send_event(
                    &mut writer,
                    DeviceEvent::Telemetry(DeviceTelemetry {
                        device_id: settings.device_id.clone(),
                        uptime_seconds: 0,
                        free_memory_bytes: 0,
                        temperature_celsius: None,
                        wifi_rssi_dbm: None,
                        current_message_id: active.as_ref().map(|active| active.message.id.clone()),
                    }),
                ).await?;
            }
        }
    }

    Ok(())
}

async fn send_event<S>(
    writer: &mut S,
    event: DeviceEvent,
) -> Result<(), Box<dyn std::error::Error>>
where
    S: SinkExt<Message> + Unpin,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    writer
        .send(Message::Text(serde_json::to_string(&event)?.into()))
        .await?;
    Ok(())
}

fn config_from_env() -> io::Result<DisplayConfig> {
    let config = DisplayConfig {
        width: env_u16("LEDDY_MATRIX_WIDTH", 100),
        height: env_u16("LEDDY_MATRIX_HEIGHT", 10),
        brightness: env_u8("LEDDY_BRIGHTNESS", 96),
        serpentine: env_bool("LEDDY_SERPENTINE", true),
        origin: env_origin("LEDDY_PIXEL_ORIGIN", PixelOrigin::TopLeft)?,
    };
    validate_supported_config(&config)?;
    Ok(config)
}

fn validate_supported_config(config: &DisplayConfig) -> io::Result<()> {
    config
        .validate()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
    if !(RECOMMENDED_MIN_WIDTH..=RECOMMENDED_MAX_WIDTH).contains(&config.width) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "matrix width {} is outside supported range {}..={}",
                config.width, RECOMMENDED_MIN_WIDTH, RECOMMENDED_MAX_WIDTH
            ),
        ));
    }
    if !(RECOMMENDED_MIN_HEIGHT..=RECOMMENDED_MAX_HEIGHT).contains(&config.height) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "matrix height {} is outside supported range {}..={}",
                config.height, RECOMMENDED_MIN_HEIGHT, RECOMMENDED_MAX_HEIGHT
            ),
        ));
    }
    Ok(())
}

fn load_config(path: &Path, fallback: DisplayConfig) -> io::Result<DisplayConfig> {
    match fs::read(path) {
        Ok(bytes) => {
            let config: DisplayConfig = serde_json::from_slice(&bytes)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            validate_supported_config(&config)?;
            Ok(config)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(fallback),
        Err(error) => Err(error),
    }
}

fn persist_config(path: &Path, config: &DisplayConfig) -> io::Result<()> {
    validate_supported_config(config)?;
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("display-config.json");
    let temporary = path.with_file_name(format!(".{file_name}.tmp"));
    let bytes = serde_json::to_vec_pretty(config)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    fs::write(&temporary, bytes)?;
    fs::rename(temporary, path)
}

fn encode_device_frame(config: &DisplayConfig, frame: &FrameBuffer) -> Vec<u8> {
    frame
        .device_order()
        .into_iter()
        .map(|pixel| if pixel == 0 { 0 } else { config.brightness })
        .collect()
}

fn next_backoff(current: Duration) -> Duration {
    current.saturating_mul(2).min(MAX_RECONNECT_BACKOFF)
}

fn env_u64(name: &str, fallback: u64) -> u64 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(fallback)
}

fn env_u16(name: &str, fallback: u16) -> u16 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(fallback)
}

fn env_u8(name: &str, fallback: u8) -> u8 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(fallback)
}

fn env_bool(name: &str, fallback: bool) -> bool {
    match env::var(name).ok().as_deref().map(str::to_ascii_lowercase) {
        Some(value) if matches!(value.as_str(), "1" | "true" | "yes" | "on") => true,
        Some(value) if matches!(value.as_str(), "0" | "false" | "no" | "off") => false,
        _ => fallback,
    }
}

fn env_origin(name: &str, fallback: PixelOrigin) -> io::Result<PixelOrigin> {
    match env::var(name).ok().as_deref().map(str::to_ascii_lowercase) {
        None => Ok(fallback),
        Some(value) => match value.replace('-', "_").as_str() {
            "top_left" => Ok(PixelOrigin::TopLeft),
            "top_right" => Ok(PixelOrigin::TopRight),
            "bottom_left" => Ok(PixelOrigin::BottomLeft),
            "bottom_right" => Ok(PixelOrigin::BottomRight),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unsupported pixel origin {value}"),
            )),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn supported_config() -> DisplayConfig {
        DisplayConfig {
            width: 100,
            height: 5,
            brightness: 80,
            serpentine: true,
            origin: PixelOrigin::TopLeft,
        }
    }

    #[test]
    fn supported_range_is_enforced() {
        assert!(validate_supported_config(&supported_config()).is_ok());
        let mut config = supported_config();
        config.width = 99;
        assert!(validate_supported_config(&config).is_err());
        config.width = 300;
        config.height = 21;
        assert!(validate_supported_config(&config).is_err());
    }

    #[test]
    fn device_frame_applies_serpentine_order_and_brightness() {
        let config = DisplayConfig {
            width: 3,
            height: 2,
            brightness: 96,
            serpentine: true,
            origin: PixelOrigin::TopLeft,
        };
        let mut frame = FrameBuffer::new(&config);
        frame.set(0, 0, 255);
        frame.set(0, 1, 255);
        assert_eq!(encode_device_frame(&config, &frame), vec![96, 0, 0, 0, 0, 96]);
    }

    #[test]
    fn configuration_round_trips_atomically() {
        let path = env::temp_dir().join(format!(
            "leddy-rasp-pi-config-{}-{}.json",
            std::process::id(),
            Instant::now().elapsed().as_nanos()
        ));
        let config = supported_config();
        persist_config(&path, &config).expect("persist config");
        let loaded = load_config(&path, supported_config()).expect("load config");
        assert_eq!(loaded, config);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn reconnect_backoff_is_bounded() {
        assert_eq!(next_backoff(Duration::from_millis(250)), Duration::from_millis(500));
        assert_eq!(next_backoff(Duration::from_secs(20)), MAX_RECONNECT_BACKOFF);
        assert_eq!(next_backoff(MAX_RECONNECT_BACKOFF), MAX_RECONNECT_BACKOFF);
    }
}
