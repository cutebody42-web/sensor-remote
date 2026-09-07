//! Windows Media Foundation H.264 transforms. No screenshot/JPEG codec fallback.
use sensor_media::{nv12_to_rgba, pixels, DecodedFrame, EncodedFrame, MAX_ENCODED_FRAME};
use std::{
    marker::PhantomData,
    mem::ManuallyDrop,
    rc::Rc,
    time::{Duration, Instant},
};
use thiserror::Error;
use windows::{
    core::{Interface, GUID},
    Win32::{
        Media::MediaFoundation::*,
        System::{Com::*, Variant::VARIANT},
    },
};

#[derive(Debug, Error)]
pub enum CodecError {
    #[error("Windows video codec: {0}")]
    Windows(#[from] windows::core::Error),
    #[error("media validation: {0}")]
    Media(#[from] sensor_media::MediaError),
    #[error("No compatible H.264 transform is available")]
    Unavailable,
    #[error("H.264 transform deadline exceeded")]
    Timeout,
    #[error("invalid or oversized codec output")]
    Invalid,
    #[error("codec output dimensions {actual_width}x{actual_height} do not match {expected_width}x{expected_height}")]
    OutputDimensions {
        expected_width: u32,
        expected_height: u32,
        actual_width: u32,
        actual_height: u32,
    },
}

struct Runtime {
    _thread: PhantomData<Rc<()>>,
}
impl Runtime {
    fn new() -> Result<Self, CodecError> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
            if let Err(error) = MFStartup(MF_VERSION, MFSTARTUP_FULL) {
                CoUninitialize();
                return Err(error.into());
            }
        }
        Ok(Self {
            _thread: PhantomData,
        })
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        unsafe {
            let _ = MFShutdown();
            CoUninitialize();
        }
    }
}

struct Activations {
    pointer: *mut Option<IMFActivate>,
    count: u32,
}
impl Activations {
    fn enumerate(
        category: GUID,
        hardware: bool,
        input: GUID,
        output: GUID,
    ) -> Result<Self, CodecError> {
        let mut value = Self {
            pointer: std::ptr::null_mut(),
            count: 0,
        };
        let input = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: input,
        };
        let output = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: output,
        };
        unsafe {
            MFTEnumEx(
                category,
                MFT_ENUM_FLAG_SORTANDFILTER
                    | if hardware {
                        MFT_ENUM_FLAG_HARDWARE
                    } else {
                        MFT_ENUM_FLAG_SYNCMFT
                    },
                Some(&input),
                Some(&output),
                &mut value.pointer,
                &mut value.count,
            )?;
        }
        if value.count > 256 || (value.count != 0 && value.pointer.is_null()) {
            return Err(CodecError::Invalid);
        }
        Ok(value)
    }
    fn iter(&self) -> &[Option<IMFActivate>] {
        if self.count == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(self.pointer, self.count as usize) }
        }
    }
}
impl Drop for Activations {
    fn drop(&mut self) {
        if !self.pointer.is_null() {
            unsafe {
                for i in 0..self.count {
                    std::ptr::drop_in_place(self.pointer.add(i as usize));
                }
                CoTaskMemFree(Some(self.pointer.cast()));
            }
        }
    }
}

fn video_type(
    subtype: &GUID,
    width: u32,
    height: u32,
    fps: u32,
) -> Result<IMFMediaType, CodecError> {
    unsafe {
        let media = MFCreateMediaType()?;
        media.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        media.SetGUID(&MF_MT_SUBTYPE, subtype)?;
        media.SetUINT64(&MF_MT_FRAME_SIZE, ((width as u64) << 32) | height as u64)?;
        media.SetUINT64(&MF_MT_FRAME_RATE, ((fps as u64) << 32) | 1)?;
        media.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1_u64 << 32) | 1)?;
        media.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        Ok(media)
    }
}

fn sample(bytes: &[u8], timestamp: i64, duration: i64) -> Result<IMFSample, CodecError> {
    if bytes.is_empty() || bytes.len() > sensor_media::MAX_PIXELS * 4 {
        return Err(CodecError::Invalid);
    }
    unsafe {
        let sample = MFCreateSample()?;
        let buffer = MFCreateMemoryBuffer(bytes.len() as u32)?;
        let mut pointer = std::ptr::null_mut();
        buffer.Lock(&mut pointer, None, None)?;
        if pointer.is_null() {
            let _ = buffer.Unlock();
            return Err(CodecError::Invalid);
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer, bytes.len());
        buffer.Unlock()?;
        buffer.SetCurrentLength(bytes.len() as u32)?;
        sample.AddBuffer(&buffer)?;
        sample.SetSampleTime(timestamp)?;
        sample.SetSampleDuration(duration)?;
        Ok(sample)
    }
}
fn sample_bytes(sample: &IMFSample, maximum: usize) -> Result<Vec<u8>, CodecError> {
    unsafe {
        let buffer = sample.ConvertToContiguousBuffer()?;
        let length = buffer.GetCurrentLength()? as usize;
        if length > maximum {
            return Err(CodecError::Invalid);
        }
        let mut pointer = std::ptr::null_mut();
        buffer.Lock(&mut pointer, None, None)?;
        let result = if length == 0 {
            Ok(Vec::new())
        } else if pointer.is_null() {
            Err(CodecError::Invalid)
        } else {
            Ok(std::slice::from_raw_parts(pointer, length).to_vec())
        };
        buffer.Unlock()?;
        result
    }
}

struct Output(MFT_OUTPUT_DATA_BUFFER);
impl Drop for Output {
    fn drop(&mut self) {
        unsafe {
            ManuallyDrop::drop(&mut self.0.pSample);
            ManuallyDrop::drop(&mut self.0.pEvents);
        }
    }
}
struct Engine {
    transform: IMFTransform,
    events: Option<IMFMediaEventGenerator>,
    input_ready: u32,
    output_ready: u32,
}
impl Engine {
    fn new(transform: IMFTransform) -> Result<Self, CodecError> {
        let attributes = unsafe { transform.GetAttributes() }.ok();
        let asynchronous = attributes
            .as_ref()
            .is_some_and(|a| unsafe { a.GetUINT32(&MF_TRANSFORM_ASYNC) }.unwrap_or(0) != 0);
        if let Some(attributes) = &attributes {
            unsafe {
                let _ = attributes.SetUINT32(&MF_LOW_LATENCY, 1);
                if asynchronous {
                    attributes.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1)?;
                }
            }
        }
        let events = if asynchronous {
            Some(transform.cast()?)
        } else {
            None
        };
        Ok(Self {
            transform,
            events,
            input_ready: 0,
            output_ready: 0,
        })
    }
    fn start(&mut self) -> Result<(), CodecError> {
        unsafe {
            self.transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
            self.transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;
            Ok(())
        }
    }
    fn pump(&mut self) -> Result<(), CodecError> {
        if let Some(events) = &self.events {
            for _ in 0..128 {
                unsafe {
                    let event = match events.GetEvent(MF_EVENT_FLAG_NO_WAIT) {
                        Ok(event) => event,
                        Err(error) if error.code() == MF_E_NO_EVENTS_AVAILABLE => break,
                        Err(error) => return Err(error.into()),
                    };
                    event.GetStatus()?.ok()?;
                    match event.GetType()? {
                        value if value == METransformNeedInput.0 as u32 => {
                            self.input_ready = self.input_ready.saturating_add(1)
                        }
                        value if value == METransformHaveOutput.0 as u32 => {
                            self.output_ready = self.output_ready.saturating_add(1)
                        }
                        _ => (),
                    }
                }
            }
        }
        Ok(())
    }
    fn input(&mut self, sample: &IMFSample) -> Result<(), CodecError> {
        if self.events.is_some() {
            let deadline = Instant::now() + Duration::from_secs(2);
            while self.input_ready == 0 {
                self.pump()?;
                if Instant::now() > deadline {
                    return Err(CodecError::Timeout);
                }
                if self.input_ready == 0 {
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
            self.input_ready -= 1;
        }
        unsafe {
            self.transform.ProcessInput(0, sample, 0)?;
        }
        Ok(())
    }
    fn output(&mut self, maximum: usize) -> Result<Option<IMFSample>, CodecError> {
        self.pump()?;
        if self.events.is_some() && self.output_ready == 0 {
            return Ok(None);
        }
        if self.events.is_some() {
            self.output_ready -= 1;
        }
        unsafe {
            let info = self.transform.GetOutputStreamInfo(0)?;
            let output_sample = if info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 != 0 {
                None
            } else {
                let size = (info.cbSize as usize).max(4096);
                if size > maximum {
                    return Err(CodecError::Invalid);
                }
                let sample = MFCreateSample()?;
                sample.AddBuffer(&MFCreateMemoryBuffer(size as u32)?)?;
                Some(sample)
            };
            let mut output = Output(MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: ManuallyDrop::new(output_sample),
                dwStatus: 0,
                pEvents: ManuallyDrop::new(None),
            });
            let mut status = 0;
            match self
                .transform
                .ProcessOutput(0, std::slice::from_mut(&mut output.0), &mut status)
            {
                Ok(()) => Ok(ManuallyDrop::take(&mut output.0.pSample).inspect(|_| {
                    output.0.pSample = ManuallyDrop::new(None);
                })),
                Err(error) if error.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => Ok(None),
                Err(error) => Err(error.into()),
            }
        }
    }
}
impl Drop for Engine {
    fn drop(&mut self) {
        unsafe {
            let _ = self
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
            let _ = self.transform.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0);
            let _ = self
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0);
            if let Ok(shutdown) = self.transform.cast::<IMFShutdown>() {
                let _ = shutdown.Shutdown();
            }
        }
    }
}

pub struct H264Encoder {
    engine: Engine,
    pub name: String,
    pub hardware: bool,
    width: u32,
    height: u32,
    fps: u32,
    _runtime: Runtime,
}
impl H264Encoder {
    pub fn new(
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u32,
        prefer_hardware: bool,
    ) -> Result<Self, CodecError> {
        pixels(width, height)?;
        if !width.is_multiple_of(2)
            || !height.is_multiple_of(2)
            || fps == 0
            || fps > 60
            || !(100_000..=50_000_000).contains(&bitrate)
        {
            return Err(CodecError::Invalid);
        }
        let runtime = Runtime::new()?;
        for hardware in [true, false] {
            if hardware && !prefer_hardware {
                continue;
            }
            let list = match Activations::enumerate(
                MFT_CATEGORY_VIDEO_ENCODER,
                hardware,
                MFVideoFormat_NV12,
                MFVideoFormat_H264,
            ) {
                Ok(list) => list,
                Err(_) if hardware => continue,
                Err(error) => return Err(error),
            };
            for activation in list.iter().iter().flatten() {
                let attempt = (|| -> Result<(Engine, String), CodecError> {
                    unsafe {
                        let transform: IMFTransform = activation.ActivateObject()?;
                        let mut engine = Engine::new(transform)?;
                        if let Ok(codec) = engine.transform.cast::<ICodecAPI>() {
                            let _ =
                                codec.SetValue(&CODECAPI_AVLowLatencyMode, &VARIANT::from(true));
                            let _ = codec.SetValue(
                                &CODECAPI_AVEncMPVDefaultBPictureCount,
                                &VARIANT::from(0_u32),
                            );
                            let _ =
                                codec.SetValue(&CODECAPI_AVEncMPVGOPSize, &VARIANT::from(fps * 2));
                            let _ = codec.SetValue(
                                &CODECAPI_AVEncCommonQualityVsSpeed,
                                &VARIANT::from(0_u32),
                            );
                        }
                        let output = video_type(&MFVideoFormat_H264, width, height, fps)?;
                        output.SetUINT32(&MF_MT_AVG_BITRATE, bitrate)?;
                        output.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_Base.0 as u32)?;
                        engine.transform.SetOutputType(0, &output, 0)?;
                        let input = video_type(&MFVideoFormat_NV12, width, height, fps)?;
                        input.SetUINT32(&MF_MT_DEFAULT_STRIDE, width)?;
                        input.SetUINT32(&MF_MT_SAMPLE_SIZE, width * height * 3 / 2)?;
                        engine.transform.SetInputType(0, &input, 0)?;
                        let mut text = [0_u16; 256];
                        let mut length = 0;
                        let name = if activation
                            .GetString(&MFT_FRIENDLY_NAME_Attribute, &mut text, Some(&mut length))
                            .is_ok()
                        {
                            String::from_utf16_lossy(&text[..(length as usize).min(text.len())])
                        } else {
                            "Windows H.264 MFT".into()
                        };
                        engine.start()?;
                        Ok((engine, name))
                    }
                })();
                if let Ok((engine, name)) = attempt {
                    return Ok(Self {
                        engine,
                        name,
                        hardware,
                        width,
                        height,
                        fps,
                        _runtime: runtime,
                    });
                }
            }
        }
        Err(CodecError::Unavailable)
    }
    pub fn encode(
        &mut self,
        nv12: &[u8],
        timestamp_100ns: i64,
    ) -> Result<Vec<EncodedFrame>, CodecError> {
        if nv12.len() != self.width as usize * self.height as usize * 3 / 2 || timestamp_100ns < 0 {
            return Err(CodecError::Invalid);
        }
        let mut packets = self.available()?;
        self.engine.input(&sample(
            nv12,
            timestamp_100ns,
            10_000_000 / self.fps as i64,
        )?)?;
        packets.extend(self.available()?);
        Ok(packets)
    }
    pub fn available(&mut self) -> Result<Vec<EncodedFrame>, CodecError> {
        let mut result = Vec::new();
        for _ in 0..16 {
            let sample = match self.engine.output(MAX_ENCODED_FRAME) {
                Ok(value) => value,
                Err(CodecError::Windows(error)) if error.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                    let mut selected = None;
                    for index in 0..32 {
                        // Some hardware encoders announce their final SPS/PPS
                        // media type only after consuming the first input.
                        unsafe {
                            let media = match self.engine.transform.GetOutputAvailableType(0, index)
                            {
                                Ok(media) => media,
                                Err(_) => break,
                            };
                            if media.GetGUID(&MF_MT_SUBTYPE)? != MFVideoFormat_H264 {
                                continue;
                            }
                            let size = media.GetUINT64(&MF_MT_FRAME_SIZE)?;
                            if (size >> 32) as u32 != self.width || size as u32 != self.height {
                                continue;
                            }
                            if self.engine.transform.SetOutputType(0, &media, 0).is_ok() {
                                selected = Some(());
                                break;
                            }
                        }
                    }
                    selected.ok_or(CodecError::Invalid)?;
                    continue;
                }
                Err(error) => return Err(error),
            };
            let Some(sample) = sample else {
                break;
            };
            let bytes = sample_bytes(&sample, MAX_ENCODED_FRAME)?;
            if !bytes.is_empty() {
                result.push(EncodedFrame {
                    bytes,
                    timestamp_100ns: unsafe { sample.GetSampleTime()? },
                    keyframe: unsafe { sample.GetUINT32(&MFSampleExtension_CleanPoint) }
                        .unwrap_or(0)
                        != 0,
                });
            }
        }
        Ok(result)
    }
    pub fn drain(&mut self) -> Result<Vec<EncodedFrame>, CodecError> {
        unsafe {
            self.engine
                .transform
                .ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0)?;
        }
        let mut output = Vec::new();
        let until = Instant::now() + Duration::from_millis(500);
        loop {
            output.extend(self.available()?);
            if self.engine.events.is_none() || Instant::now() >= until {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(output)
    }
}

pub struct H264Decoder {
    engine: Engine,
    width: u32,
    height: u32,
    fps: u32,
    _runtime: Runtime,
}
impl H264Decoder {
    pub fn new(width: u32, height: u32, fps: u32) -> Result<Self, CodecError> {
        pixels(width, height)?;
        if fps == 0 || fps > 60 {
            return Err(CodecError::Invalid);
        }
        let runtime = Runtime::new()?;
        let list = Activations::enumerate(
            MFT_CATEGORY_VIDEO_DECODER,
            false,
            MFVideoFormat_H264,
            MFVideoFormat_NV12,
        )?;
        for activation in list.iter().iter().flatten() {
            let attempt = (|| -> Result<Engine, CodecError> {
                unsafe {
                    let mut engine = Engine::new(activation.ActivateObject()?)?;
                    engine.transform.SetInputType(
                        0,
                        &video_type(&MFVideoFormat_H264, width, height, fps)?,
                        0,
                    )?;
                    engine.transform.SetOutputType(
                        0,
                        &video_type(&MFVideoFormat_NV12, width, height, fps)?,
                        0,
                    )?;
                    engine.start()?;
                    Ok(engine)
                }
            })();
            if let Ok(engine) = attempt {
                return Ok(Self {
                    engine,
                    width,
                    height,
                    fps,
                    _runtime: runtime,
                });
            }
        }
        Err(CodecError::Unavailable)
    }
    pub fn decode(&mut self, frame: &EncodedFrame) -> Result<Vec<DecodedFrame>, CodecError> {
        if frame.bytes.is_empty() || frame.bytes.len() > MAX_ENCODED_FRAME {
            return Err(CodecError::Invalid);
        }
        self.engine.input(&sample(
            &frame.bytes,
            frame.timestamp_100ns,
            10_000_000 / self.fps as i64,
        )?)?;
        let mut output = Vec::new();
        for _ in 0..16 {
            let value = match self.engine.output(sensor_media::MAX_PIXELS * 4) {
                Ok(value) => value,
                Err(CodecError::Windows(error)) if error.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                    let mut selected = None;
                    for index in 0..32 {
                        unsafe {
                            let media = match self.engine.transform.GetOutputAvailableType(0, index)
                            {
                                Ok(media) => media,
                                Err(_) => break,
                            };
                            if media.GetGUID(&MF_MT_SUBTYPE)? == MFVideoFormat_NV12 {
                                let size = media.GetUINT64(&MF_MT_FRAME_SIZE)?;
                                if (size >> 32) as u32 != self.width || size as u32 != self.height {
                                    return Err(CodecError::OutputDimensions {
                                        expected_width: self.width,
                                        expected_height: self.height,
                                        actual_width: (size >> 32) as u32,
                                        actual_height: size as u32,
                                    });
                                }
                                selected = Some(media);
                                break;
                            }
                        }
                    }
                    unsafe {
                        self.engine.transform.SetOutputType(
                            0,
                            &selected.ok_or(CodecError::Invalid)?,
                            0,
                        )?;
                    }
                    continue;
                }
                Err(error) => return Err(error),
            };
            let Some(sample) = value else {
                break;
            };
            let media = unsafe { self.engine.transform.GetOutputCurrentType(0)? };
            let size = unsafe { media.GetUINT64(&MF_MT_FRAME_SIZE)? };
            if (size >> 32) as u32 != self.width || size as u32 != self.height {
                return Err(CodecError::OutputDimensions {
                    expected_width: self.width,
                    expected_height: self.height,
                    actual_width: (size >> 32) as u32,
                    actual_height: size as u32,
                });
            }
            let stride =
                unsafe { media.GetUINT32(&MF_MT_DEFAULT_STRIDE) }.unwrap_or(self.width) as usize;
            let bytes = sample_bytes(&sample, sensor_media::MAX_PIXELS * 4)?;
            output.push(nv12_to_rgba(&bytes, self.width, self.height, stride)?);
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_windows_h264_encode_decode_preserves_test_colors() {
        let (width, height) = (256, 144);
        let bgra = sensor_media::BgraFrame {
            width,
            height,
            bytes: [20, 80, 200, 255].repeat((width * height) as usize),
        };
        let raw = sensor_media::bgra_to_nv12(&bgra).unwrap();
        let mut encoder = H264Encoder::new(width, height, 30, 2_000_000, false).unwrap();
        assert!(!encoder.hardware);
        let mut packets = Vec::new();
        for index in 0..8 {
            packets.extend(encoder.encode(&raw, index * 333_333).unwrap());
        }
        packets.extend(encoder.drain().unwrap());
        assert!(!packets.is_empty());
        assert!(packets.iter().any(|p| p.keyframe));
        let mut decoder = H264Decoder::new(width, height, 30).unwrap();
        let mut frames = Vec::new();
        for packet in &packets {
            frames.extend(decoder.decode(packet).unwrap());
        }
        assert!(!frames.is_empty());
        let pixel = &frames[0].rgba[(70 * 256 + 100) * 4..][..4];
        for (actual, expected) in pixel.iter().zip([200, 80, 20, 255]) {
            assert!(
                (*actual as i32 - expected).abs() < 12,
                "decoded color {pixel:?}"
            );
        }
        eprintln!(
            "Actual codec: {}; {} encoded packets; {} decoded frames",
            encoder.name,
            packets.len(),
            frames.len()
        );
    }
}
