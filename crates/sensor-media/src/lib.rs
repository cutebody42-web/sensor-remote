//! Bounded remote video/input wire types and independently testable pixel transforms.
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const MAX_WIDTH: u32 = 4096;
pub const MAX_HEIGHT: u32 = 4096;
pub const MAX_PIXELS: usize = 4096 * 2160;
pub const MAX_ENCODED_FRAME: usize = 8 * 1024 * 1024;
pub const VIDEO_FRAGMENT: usize = 128 * 1024;
pub const MAX_CLIPBOARD_BYTES: usize = 64 * 1024;

pub fn validate_clipboard(text: &str) -> Result<(), MediaError> {
    if text.len() > MAX_CLIPBOARD_BYTES || text.contains('\0') {
        return Err(MediaError::Input);
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum MediaError {
    #[error("invalid or unsupported video dimensions")]
    Dimensions,
    #[error("invalid video frame buffer or fragment sequence")]
    Frame,
    #[error("invalid remote input")]
    Input,
}

pub fn pixels(width: u32, height: u32) -> Result<usize, MediaError> {
    let count = (width as usize)
        .checked_mul(height as usize)
        .ok_or(MediaError::Dimensions)?;
    if width == 0 || height == 0 || width > MAX_WIDTH || height > MAX_HEIGHT || count > MAX_PIXELS {
        return Err(MediaError::Dimensions);
    }
    Ok(count)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Display {
    pub index: u32,
    pub name: String,
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
}
impl Display {
    pub fn validate(&self) -> Result<(), MediaError> {
        pixels(self.width, self.height)?;
        if self.index > 63 || self.name.len() > 256 {
            return Err(MediaError::Dimensions);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Input {
    Move { x: u32, y: u32 },
    Button { button: MouseButton, down: bool },
    Wheel { delta: i32, horizontal: bool },
    Key { virtual_key: u16, down: bool },
    Text(String),
    ReleaseAll,
}
impl Input {
    pub fn validate(&self, display: &Display) -> Result<(), MediaError> {
        match self {
            Self::Move { x, y } if *x >= display.width || *y >= display.height => {
                Err(MediaError::Input)
            }
            Self::Key { virtual_key, .. } if *virtual_key == 0 || *virtual_key > 254 => {
                Err(MediaError::Input)
            }
            Self::Wheel { delta, .. } if delta.unsigned_abs() > 120 * 100 => Err(MediaError::Input),
            Self::Text(text) if text.len() > 1024 || text.chars().any(char::is_control) => {
                Err(MediaError::Input)
            }
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VideoFormat {
    pub generation: u64,
    pub display: Display,
    pub width: u32,
    pub height: u32,
    pub fps_limit: u32,
    pub bitrate: u32,
    pub encoder: String,
    pub hardware: bool,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum VideoProfile {
    #[default]
    Balanced,
    SharpText,
    Smooth,
}
impl VideoProfile {
    pub fn fps(self) -> u32 {
        match self {
            Self::Balanced => 30,
            Self::SharpText => 15,
            Self::Smooth => 60,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Balanced => "Balanced · up to 720p / 30 fps",
            Self::SharpText => "Sharp text · up to 1080p / 15 fps",
            Self::Smooth => "Smooth · up to 720p / 60 fps",
        }
    }
    pub fn dimensions(self, width: u32, height: u32) -> Result<(u32, u32), MediaError> {
        let (width, height) = stream_size(width, height)?;
        let (max_w, max_h) = if self == Self::SharpText {
            (1920.0, 1080.0)
        } else {
            (1280.0, 720.0)
        };
        let scale = (max_w / width as f64).min(max_h / height as f64).min(1.0);
        Ok((
            ((width as f64 * scale) as u32 / 16 * 16).max(16),
            ((height as f64 * scale) as u32 / 16 * 16).max(16),
        ))
    }
}
impl VideoFormat {
    pub fn validate(&self) -> Result<(), MediaError> {
        self.display.validate()?;
        pixels(self.width, self.height)?;
        if !self.width.is_multiple_of(2)
            || !self.height.is_multiple_of(2)
            || self.fps_limit == 0
            || self.fps_limit > 60
            || self.bitrate > 50_000_000
            || self.bitrate < 100_000
            || self.encoder.len() > 256
        {
            return Err(MediaError::Dimensions);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum DesktopMessage {
    Displays(Vec<Display>),
    Format(VideoFormat),
    Fragment {
        generation: u64,
        sequence: u64,
        timestamp_100ns: i64,
        keyframe: bool,
        offset: u32,
        total: u32,
        bytes: Vec<u8>,
    },
    Input {
        generation: u64,
        event: Input,
    },
    SelectDisplay(u32),
    Cursor {
        generation: u64,
        x: i32,
        y: i32,
        visible: bool,
    },
    Ping(u64),
    Pong(u64),
    Close,
    Error(String),
    // Appended variants preserve existing postcard discriminants.
    ClipboardText(String),
    // Optional 0.3.4+ viewer command. Both endpoints must be updated.
    SelectVideoProfile(VideoProfile),
}

#[derive(Clone, Debug)]
pub struct BgraFrame {
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
}
#[derive(Clone, Debug)]
pub struct DecodedFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
#[derive(Clone, Debug)]
pub struct EncodedFrame {
    pub timestamp_100ns: i64,
    pub keyframe: bool,
    pub bytes: Vec<u8>,
}

/// Bound the initial software conversion path; stream dimensions are multiples
/// of 16 for portable NV12 decoder pitch. Input coordinates remain native-sized.
pub fn stream_size(width: u32, height: u32) -> Result<(u32, u32), MediaError> {
    pixels(width, height)?;
    if width < 16 || height < 16 {
        return Err(MediaError::Dimensions);
    }
    let scale = (1920.0 / width as f64).min(1088.0 / height as f64).min(1.0);
    Ok((
        ((width as f64 * scale) as u32 / 16 * 16).max(16),
        ((height as f64 * scale) as u32 / 16 * 16).max(16),
    ))
}

pub fn rotate_scale_bgra(
    frame: BgraFrame,
    clockwise: u32,
    width: u32,
    height: u32,
) -> Result<BgraFrame, MediaError> {
    let count = pixels(frame.width, frame.height)?;
    if frame.bytes.len() != count * 4 || ![0, 90, 180, 270].contains(&clockwise) {
        return Err(MediaError::Frame);
    }
    let count = pixels(width, height)?;
    if clockwise == 0 && width == frame.width && height == frame.height {
        return Ok(frame);
    }
    let (oriented_w, oriented_h) = if clockwise == 90 || clockwise == 270 {
        (frame.height, frame.width)
    } else {
        (frame.width, frame.height)
    };
    let mut bytes = vec![0; count * 4];
    for y in 0..height {
        for x in 0..width {
            let ox = x as u64 * oriented_w as u64 / width as u64;
            let oy = y as u64 * oriented_h as u64 / height as u64;
            let (sx, sy) = match clockwise {
                0 => (ox as u32, oy as u32),
                90 => (oy as u32, frame.height - 1 - ox as u32),
                180 => (frame.width - 1 - ox as u32, frame.height - 1 - oy as u32),
                _ => (frame.width - 1 - oy as u32, ox as u32),
            };
            let input = ((sy * frame.width + sx) * 4) as usize;
            let output = ((y * width + x) * 4) as usize;
            bytes[output..output + 4].copy_from_slice(&frame.bytes[input..input + 4]);
        }
    }
    Ok(BgraFrame {
        width,
        height,
        bytes,
    })
}

pub fn bgra_to_nv12(frame: &BgraFrame) -> Result<Vec<u8>, MediaError> {
    let count = pixels(frame.width, frame.height)?;
    if !frame.width.is_multiple_of(2)
        || !frame.height.is_multiple_of(2)
        || frame.bytes.len() != count * 4
    {
        return Err(MediaError::Frame);
    }
    let mut out = vec![0; count * 3 / 2];
    for (i, p) in frame.bytes.as_chunks::<4>().0.iter().enumerate() {
        let (b, g, r) = (p[0] as i32, p[1] as i32, p[2] as i32);
        out[i] = (((66 * r + 129 * g + 25 * b + 128) >> 8) + 16).clamp(0, 255) as u8;
    }
    let w = frame.width as usize;
    for y in (0..frame.height as usize).step_by(2) {
        for x in (0..w).step_by(2) {
            let (mut r, mut g, mut b) = (0, 0, 0);
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let p = ((y + dy) * w + x + dx) * 4;
                b += frame.bytes[p] as i32;
                g += frame.bytes[p + 1] as i32;
                r += frame.bytes[p + 2] as i32;
            }
            r /= 4;
            g /= 4;
            b /= 4;
            let index = count + (y / 2) * w + x;
            out[index] = (((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128).clamp(0, 255) as u8;
            out[index + 1] = (((112 * r - 94 * g - 18 * b + 128) >> 8) + 128).clamp(0, 255) as u8;
        }
    }
    Ok(out)
}

pub fn nv12_to_rgba(
    bytes: &[u8],
    width: u32,
    height: u32,
    stride: usize,
) -> Result<DecodedFrame, MediaError> {
    let count = pixels(width, height)?;
    if !width.is_multiple_of(2)
        || !height.is_multiple_of(2)
        || stride < width as usize
        || stride > MAX_WIDTH as usize * 4
        || bytes.len() < stride * height as usize * 3 / 2
    {
        return Err(MediaError::Frame);
    }
    let mut rgba = vec![0; count * 4];
    for y in 0..height as usize {
        for x in 0..width as usize {
            let c = (bytes[y * stride + x] as i32 - 16).max(0);
            let chroma = stride * height as usize + (y / 2) * stride + (x / 2) * 2;
            let d = bytes[chroma] as i32 - 128;
            let e = bytes[chroma + 1] as i32 - 128;
            let p = (y * width as usize + x) * 4;
            rgba[p] = ((298 * c + 409 * e + 128) >> 8).clamp(0, 255) as u8;
            rgba[p + 1] = ((298 * c - 100 * d - 208 * e + 128) >> 8).clamp(0, 255) as u8;
            rgba[p + 2] = ((298 * c + 516 * d + 128) >> 8).clamp(0, 255) as u8;
            rgba[p + 3] = 255;
        }
    }
    Ok(DecodedFrame {
        width,
        height,
        rgba,
    })
}

/// The network may hold at most one bounded compressed frame under assembly.
#[derive(Default)]
pub struct Assembler {
    frame: Option<(u64, u64, i64, bool, usize, Vec<u8>)>,
}
impl Assembler {
    pub fn clear(&mut self) {
        self.frame = None;
    }
    pub fn push(&mut self, message: DesktopMessage) -> Result<Option<EncodedFrame>, MediaError> {
        let result = self.push_inner(message);
        if result.is_err() {
            self.clear();
        }
        result
    }
    fn push_inner(&mut self, message: DesktopMessage) -> Result<Option<EncodedFrame>, MediaError> {
        let DesktopMessage::Fragment {
            generation,
            sequence,
            timestamp_100ns,
            keyframe,
            offset,
            total,
            bytes,
        } = message
        else {
            return Err(MediaError::Frame);
        };
        if total == 0
            || total as usize > MAX_ENCODED_FRAME
            || bytes.is_empty()
            || bytes.len() > VIDEO_FRAGMENT
        {
            return Err(MediaError::Frame);
        }
        if self.frame.is_none() {
            if offset != 0 {
                return Err(MediaError::Frame);
            }
            self.frame = Some((
                generation,
                sequence,
                timestamp_100ns,
                keyframe,
                total as usize,
                Vec::with_capacity(total as usize),
            ));
        }
        let (g, s, time, key, length, buffer) = self.frame.as_mut().ok_or(MediaError::Frame)?;
        if *g != generation
            || *s != sequence
            || *time != timestamp_100ns
            || *key != keyframe
            || *length != total as usize
            || offset as usize != buffer.len()
            || buffer.len() + bytes.len() > *length
        {
            return Err(MediaError::Frame);
        }
        buffer.extend_from_slice(&bytes);
        if buffer.len() != *length {
            return Ok(None);
        }
        let (_, _, timestamp_100ns, keyframe, _, bytes) =
            self.frame.take().ok_or(MediaError::Frame)?;
        Ok(Some(EncodedFrame {
            timestamp_100ns,
            keyframe,
            bytes,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn video_profiles_are_bounded_and_never_upscale_small_displays() {
        for profile in [
            VideoProfile::Balanced,
            VideoProfile::SharpText,
            VideoProfile::Smooth,
        ] {
            assert_eq!(profile.dimensions(960, 720).unwrap(), (960, 720));
            let (width, height) = profile.dimensions(3840, 2160).unwrap();
            assert!(
                width <= 1920
                    && height <= 1080
                    && width.is_multiple_of(16)
                    && height.is_multiple_of(16)
            );
            assert!(profile.fps() <= 60);
        }
        assert_eq!(VideoProfile::Smooth.fps(), 60);
        assert_eq!(
            VideoProfile::SharpText.dimensions(1920, 1080).unwrap(),
            (1920, 1072)
        );
    }
    #[test]
    fn clipboard_is_utf8_bounded_and_rejects_embedded_nul() {
        assert!(validate_clipboard("").is_ok());
        assert!(validate_clipboard("Hello\nمرحبا\n日本語").is_ok());
        assert!(validate_clipboard(&"x".repeat(MAX_CLIPBOARD_BYTES)).is_ok());
        assert!(validate_clipboard(&"x".repeat(MAX_CLIPBOARD_BYTES + 1)).is_err());
        assert!(validate_clipboard(&"é".repeat(MAX_CLIPBOARD_BYTES)).is_err());
        assert!(validate_clipboard("hello\0world").is_err());
    }
    #[test]
    fn bt601_black_white_and_red_round_trip() {
        for color in [[0, 0, 0, 255], [255, 255, 255, 255], [0, 0, 255, 255]] {
            let frame = BgraFrame {
                width: 16,
                height: 16,
                bytes: color.repeat(256),
            };
            let nv12 = bgra_to_nv12(&frame).unwrap();
            let output = nv12_to_rgba(&nv12, 16, 16, 16).unwrap();
            for p in output.rgba.as_chunks::<4>().0 {
                for (actual, expected) in p[..3].iter().zip([color[2], color[1], color[0]]) {
                    assert!((*actual as i32 - expected as i32).abs() <= 3);
                }
            }
        }
        assert!(pixels(u32::MAX, u32::MAX).is_err());
        assert!(nv12_to_rgba(&[], 1920, 1080, 1920).is_err());
    }
    #[test]
    fn fragments_are_bounded_ordered_and_reset_on_failure() {
        let fragment = |offset, total, bytes| DesktopMessage::Fragment {
            generation: 1,
            sequence: 0,
            timestamp_100ns: 0,
            keyframe: true,
            offset,
            total,
            bytes,
        };
        let mut assembler = Assembler::default();
        assert!(assembler.push(fragment(0, u32::MAX, vec![1])).is_err());
        assert!(assembler
            .push(fragment(0, 4, vec![1, 2]))
            .unwrap()
            .is_none());
        assert!(assembler.push(fragment(3, 4, vec![3])).is_err());
        assert_eq!(
            assembler
                .push(fragment(0, 2, vec![4, 5]))
                .unwrap()
                .unwrap()
                .bytes,
            [4, 5]
        );
    }

    #[test]
    fn rotation_and_capture_scaling_preserve_pixel_orientation() {
        let source = BgraFrame {
            width: 2,
            height: 2,
            bytes: [1_u8, 2, 3, 4]
                .into_iter()
                .flat_map(|p| [p, 0, 0, 255])
                .collect(),
        };
        let rotated = rotate_scale_bgra(source, 90, 2, 2).unwrap();
        assert_eq!(
            rotated
                .bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| p[0])
                .collect::<Vec<_>>(),
            [3, 1, 4, 2]
        );
        assert_eq!(stream_size(1920, 1080).unwrap(), (1920, 1072));
        assert!(Input::Key {
            virtual_key: 65535,
            down: true
        }
        .validate(&Display {
            index: 0,
            name: "test".into(),
            left: 0,
            top: 0,
            width: 100,
            height: 100
        })
        .is_err());
    }
}
