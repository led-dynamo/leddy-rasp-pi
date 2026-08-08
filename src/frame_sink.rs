use leddy_interfaces::{DisplayConfig, PixelOrigin};
use leddy_lib::FrameBuffer;
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
};

const HUB75_PACKET_MAGIC: &[u8; 8] = b"LEDDYF01";
const HUB75_PACKET_VERSION: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameStats {
    pub pixels: usize,
    pub lit_pixels: usize,
}

#[derive(Debug)]
pub enum FrameSink {
    Snapshot(SnapshotSink),
    Hub75(Hub75Sink),
}

impl FrameSink {
    pub fn new(snapshot_path: Option<PathBuf>, hub75_helper: Option<PathBuf>) -> io::Result<Self> {
        match hub75_helper {
            Some(helper) => Ok(Self::Hub75(Hub75Sink::spawn(&helper)?)),
            None => Ok(Self::Snapshot(SnapshotSink::new(snapshot_path))),
        }
    }

    pub fn present(
        &mut self,
        config: &DisplayConfig,
        frame: &FrameBuffer,
    ) -> io::Result<FrameStats> {
        match self {
            Self::Snapshot(sink) => sink.present(config, frame),
            Self::Hub75(sink) => sink.present(config, frame),
        }
    }

    pub fn clear(&mut self, config: &DisplayConfig) -> io::Result<()> {
        match self {
            Self::Snapshot(sink) => sink.clear(config),
            Self::Hub75(sink) => sink.clear(config),
        }
    }
}

#[derive(Debug)]
pub struct SnapshotSink {
    snapshot_path: Option<PathBuf>,
}

impl SnapshotSink {
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
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, pixels)
    }
}

#[derive(Debug)]
pub struct Hub75Sink {
    child: Child,
    stdin: ChildStdin,
}

impl Hub75Sink {
    fn spawn(helper: &Path) -> io::Result<Self> {
        let mut child = Command::new(helper)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .env("LEDDY_HUB75_PROTOCOL", "LEDDYF01")
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("HUB75 helper did not expose stdin"))?;
        Ok(Self { child, stdin })
    }

    fn present(&mut self, config: &DisplayConfig, frame: &FrameBuffer) -> io::Result<FrameStats> {
        self.ensure_running()?;
        let pixels = frame.row_major();
        write_hub75_packet(&mut self.stdin, config, pixels)?;
        Ok(FrameStats {
            pixels: pixels.len(),
            lit_pixels: pixels.iter().filter(|pixel| **pixel != 0).count(),
        })
    }

    fn clear(&mut self, config: &DisplayConfig) -> io::Result<()> {
        self.ensure_running()?;
        write_hub75_packet(&mut self.stdin, config, &vec![0; config.pixel_count()])
    }

    fn ensure_running(&mut self) -> io::Result<()> {
        match self.child.try_wait()? {
            None => Ok(()),
            Some(status) => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                format!("HUB75 helper exited with {status}"),
            )),
        }
    }
}

impl Drop for Hub75Sink {
    fn drop(&mut self) {
        let _ = self.stdin.flush();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn encode_device_frame(config: &DisplayConfig, frame: &FrameBuffer) -> Vec<u8> {
    frame
        .device_order()
        .into_iter()
        .map(|pixel| if pixel == 0 { 0 } else { config.brightness })
        .collect()
}

fn write_hub75_packet<W: Write>(
    writer: &mut W,
    config: &DisplayConfig,
    pixels: &[u8],
) -> io::Result<()> {
    if pixels.len() != config.pixel_count() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "HUB75 frame length does not match display configuration",
        ));
    }
    let payload_len = u32::try_from(pixels.len()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "HUB75 frame payload exceeds protocol limit",
        )
    })?;

    writer.write_all(HUB75_PACKET_MAGIC)?;
    writer.write_all(&[HUB75_PACKET_VERSION])?;
    writer.write_all(&[origin_code(config.origin)])?;
    writer.write_all(&[u8::from(config.serpentine)])?;
    writer.write_all(&[config.brightness])?;
    writer.write_all(&config.width.to_le_bytes())?;
    writer.write_all(&config.height.to_le_bytes())?;
    writer.write_all(&payload_len.to_le_bytes())?;
    writer.write_all(pixels)?;
    writer.flush()
}

fn origin_code(origin: PixelOrigin) -> u8 {
    match origin {
        PixelOrigin::TopLeft => 0,
        PixelOrigin::TopRight => 1,
        PixelOrigin::BottomLeft => 2,
        PixelOrigin::BottomRight => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> DisplayConfig {
        DisplayConfig {
            width: 3,
            height: 2,
            brightness: 96,
            serpentine: true,
            origin: PixelOrigin::TopLeft,
        }
    }

    #[test]
    fn snapshot_encoding_preserves_direct_led_wiring_contract() {
        let config = config();
        let mut frame = FrameBuffer::new(&config);
        frame.set(0, 0, 255);
        frame.set(0, 1, 255);
        assert_eq!(
            encode_device_frame(&config, &frame),
            vec![96, 0, 0, 0, 0, 96]
        );
    }

    #[test]
    fn hub75_packet_keeps_logical_row_major_pixels_and_wiring_metadata() {
        let mut config = config();
        config.origin = PixelOrigin::BottomRight;
        let pixels = vec![1, 2, 3, 4, 5, 6];
        let mut packet = Vec::new();
        write_hub75_packet(&mut packet, &config, &pixels).expect("packet");

        assert_eq!(&packet[0..8], HUB75_PACKET_MAGIC);
        assert_eq!(packet[8], HUB75_PACKET_VERSION);
        assert_eq!(packet[9], 3);
        assert_eq!(packet[10], 1);
        assert_eq!(packet[11], 96);
        assert_eq!(u16::from_le_bytes([packet[12], packet[13]]), 3);
        assert_eq!(u16::from_le_bytes([packet[14], packet[15]]), 2);
        assert_eq!(u32::from_le_bytes([packet[16], packet[17], packet[18], packet[19]]), 6);
        assert_eq!(&packet[20..], pixels.as_slice());
    }

    #[test]
    fn hub75_packet_rejects_mismatched_frame_length() {
        let mut packet = Vec::new();
        let error = write_hub75_packet(&mut packet, &config(), &[1, 2, 3]).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }
}
