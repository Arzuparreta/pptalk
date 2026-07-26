//! Native `GStreamer` media discovery, strict quality policy and capture pipelines.

use std::{collections::BTreeMap, sync::Mutex};

use async_trait::async_trait;
use gst::prelude::*;
use gstreamer as gst;
use gstreamer_app as gst_app;
use pptalk_protocol::{DeviceId, MediaDatagram, MediaKind, QualityMode, QualityProfile};

#[derive(Debug)]
pub struct JitterBuffer {
    next_sequence: u64,
    max_depth: usize,
    queued: BTreeMap<u64, MediaDatagram>,
    dropped: u64,
}

impl JitterBuffer {
    pub fn new(first_sequence: u64, max_depth: usize) -> Self {
        Self {
            next_sequence: first_sequence,
            max_depth: max_depth.max(1),
            queued: BTreeMap::new(),
            dropped: 0,
        }
    }

    pub fn push(&mut self, packet: MediaDatagram) -> Vec<MediaDatagram> {
        if packet.sequence < self.next_sequence {
            self.dropped += 1;
            return vec![];
        }
        self.queued.entry(packet.sequence).or_insert(packet);
        if self.queued.len() > self.max_depth
            && let Some((&first, _)) = self.queued.first_key_value()
        {
            self.dropped += first.saturating_sub(self.next_sequence);
            self.next_sequence = first;
        }
        let mut ready = Vec::new();
        while let Some(packet) = self.queued.remove(&self.next_sequence) {
            ready.push(packet);
            self.next_sequence += 1;
        }
        ready
    }

    pub const fn dropped(&self) -> u64 {
        self.dropped
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaDevice {
    pub id: String,
    pub label: String,
    pub kind: MediaDeviceKind,
    pub is_default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MediaDeviceKind {
    AudioInput,
    AudioOutput,
    Camera,
    Screen,
    Window,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaCapabilities {
    pub devices: Vec<MediaDevice>,
    pub encoders: Vec<String>,
    pub decoders: Vec<String>,
    pub zero_copy_backends: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MediaStats {
    pub encode_ms: f32,
    pub decode_ms: f32,
    pub bitrate_kbps: u32,
    pub packet_loss: f32,
    pub jitter_ms: f32,
    pub dropped_frames: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoLimits {
    pub max_width: u16,
    pub max_height: u16,
    pub max_frames_per_second: u8,
    pub max_bitrate_kbps: u32,
}

/// Resolves Automatic profiles but preserves Manual profiles byte-for-byte.
/// This is deliberately pure so the no-silent-downgrade rule is regression tested.
pub fn resolve_quality(
    requested: &QualityProfile,
    limits: VideoLimits,
    packet_loss: f32,
) -> Result<QualityProfile, MediaError> {
    let supported = requested.width <= limits.max_width
        && requested.height <= limits.max_height
        && requested.frames_per_second <= limits.max_frames_per_second
        && requested.bitrate_kbps <= limits.max_bitrate_kbps;
    if requested.mode == QualityMode::Manual {
        return supported.then(|| requested.clone()).ok_or_else(|| {
            MediaError::Unsupported("manual quality exceeds local capability".into())
        });
    }

    let loss_percent = if packet_loss >= 0.10 {
        45
    } else if packet_loss >= 0.03 {
        70
    } else {
        100
    };
    let requested_bitrate = requested.bitrate_kbps.min(limits.max_bitrate_kbps);
    Ok(QualityProfile {
        mode: QualityMode::Automatic,
        width: requested.width.min(limits.max_width),
        height: requested.height.min(limits.max_height),
        frames_per_second: requested
            .frames_per_second
            .min(limits.max_frames_per_second),
        bitrate_kbps: requested_bitrate
            .saturating_mul(loss_percent)
            .saturating_div(100)
            .max(64),
        codec: requested.codec.clone(),
    })
}

#[async_trait]
pub trait MediaEngine: Send + Sync {
    async fn capabilities(&self) -> Result<MediaCapabilities, MediaError>;
    async fn select_device(
        &self,
        kind: MediaDeviceKind,
        device_id: Option<String>,
    ) -> Result<(), MediaError>;
    async fn publish(&self, kind: MediaKind, profile: QualityProfile) -> Result<(), MediaError>;
    async fn unpublish(&self, kind: MediaKind) -> Result<(), MediaError>;
    async fn stats(&self) -> Result<MediaStats, MediaError>;
    async fn next_rtp_packet(&self, kind: MediaKind) -> Result<Option<Vec<u8>>, MediaError>;
    async fn receive_rtp_packet(
        &self,
        source: DeviceId,
        kind: MediaKind,
        packet: Vec<u8>,
    ) -> Result<(), MediaError>;
    async fn set_receive_volume(&self, source: DeviceId, volume: f64) -> Result<(), MediaError>;
    async fn stop_receiving(&self, kind: MediaKind) -> Result<(), MediaError>;
}

/// GStreamer-backed native capture/encode engine. The resulting RTP-ready
/// streams terminate at an `appsink`; the call transport owns packet delivery.
pub struct GstMediaEngine {
    pipelines: Mutex<Vec<(MediaKind, gst::Pipeline, gst_app::AppSink)>>,
    receivers: Mutex<Vec<(DeviceId, MediaKind, gst::Pipeline, gst_app::AppSrc)>>,
    selected_devices: Mutex<BTreeMap<MediaDeviceKind, String>>,
    stats: Mutex<MediaStats>,
}

impl std::fmt::Debug for GstMediaEngine {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GstMediaEngine")
            .finish_non_exhaustive()
    }
}

impl GstMediaEngine {
    pub fn new() -> Result<Self, MediaError> {
        gst::init().map_err(|error| MediaError::Unavailable(error.to_string()))?;
        Ok(Self {
            pipelines: Mutex::new(Vec::new()),
            receivers: Mutex::new(Vec::new()),
            selected_devices: Mutex::new(BTreeMap::new()),
            stats: Mutex::new(MediaStats::default()),
        })
    }

    fn has(element: &str) -> bool {
        gst::ElementFactory::find(element).is_some()
    }

    fn first_available<'a>(candidates: &'a [&'a str]) -> Option<&'a str> {
        candidates
            .iter()
            .copied()
            .find(|candidate| Self::has(candidate))
    }

    fn media_device_id(device: &gst::Device) -> String {
        blake3::hash(format!("{}:{}", device.device_class(), device.display_name()).as_bytes())
            .to_hex()
            .to_string()
    }

    fn selected_device(&self, kind: MediaDeviceKind) -> Result<Option<gst::Device>, MediaError> {
        let selected = self
            .selected_devices
            .lock()
            .map_err(|_| MediaError::Poisoned)?
            .get(&kind)
            .cloned();
        let Some(selected) = selected else {
            return Ok(None);
        };
        let monitor = gst::DeviceMonitor::new();
        match kind {
            MediaDeviceKind::AudioInput => {
                monitor.add_filter(Some("Audio/Source"), None);
            }
            MediaDeviceKind::AudioOutput => {
                monitor.add_filter(Some("Audio/Sink"), None);
            }
            MediaDeviceKind::Camera => {
                monitor.add_filter(Some("Video/Source"), None);
            }
            MediaDeviceKind::Screen | MediaDeviceKind::Window => return Ok(None),
        }
        monitor
            .start()
            .map_err(|error| MediaError::Unavailable(error.to_string()))?;
        let found = monitor
            .devices()
            .into_iter()
            .find(|device| Self::media_device_id(device) == selected);
        monitor.stop();
        found.map(Some).ok_or_else(|| {
            MediaError::Unavailable("selected media device is no longer available".into())
        })
    }

    fn selected_capture_pipeline(
        &self,
        kind: MediaKind,
        profile: &QualityProfile,
    ) -> Result<Option<(gst::Pipeline, gst_app::AppSink)>, MediaError> {
        let device_kind = match kind {
            MediaKind::Voice | MediaKind::SystemAudio => MediaDeviceKind::AudioInput,
            MediaKind::Camera => MediaDeviceKind::Camera,
            MediaKind::Screen => return Ok(None),
        };
        let Some(device) = self.selected_device(device_kind)? else {
            return Ok(None);
        };
        let source = device
            .create_element(Some("capture_source"))
            .map_err(|error| MediaError::Pipeline(error.to_string()))?;
        let remainder = match kind {
            MediaKind::Voice | MediaKind::SystemAudio => format!(
                "audioconvert ! audioresample ! opusenc bitrate={} ! rtpopuspay pt=111 mtu=1100 ! appsink name=rtp_sink sync=false max-buffers=8 drop=true",
                profile.bitrate_kbps.saturating_mul(1000)
            ),
            MediaKind::Camera => {
                let encoder = Self::first_available(&[
                    "nvh264enc",
                    "vulkanh264enc",
                    "openh264enc",
                    "x264enc",
                ])
                .ok_or_else(|| MediaError::Unavailable("no H.264 encoder plugin".into()))?;
                let encoder = Self::encoder_description(encoder, profile.bitrate_kbps);
                format!(
                    "videoconvert ! videoscale ! videorate ! video/x-raw,width={},height={},framerate={}/1 ! {encoder} ! h264parse ! rtph264pay config-interval=-1 pt=96 mtu=1100 ! appsink name=rtp_sink sync=false max-buffers=3 drop=true",
                    profile.width, profile.height, profile.frames_per_second
                )
            }
            MediaKind::Screen => unreachable!(),
        };
        let processing = gst::parse::bin_from_description(&remainder, true)
            .map_err(|error| MediaError::Pipeline(error.to_string()))?;
        let pipeline = gst::Pipeline::new();
        pipeline
            .add_many([&source, processing.upcast_ref()])
            .map_err(|error| MediaError::Pipeline(error.to_string()))?;
        source
            .link(&processing)
            .map_err(|error| MediaError::Pipeline(error.to_string()))?;
        let sink = pipeline
            .by_name("rtp_sink")
            .ok_or_else(|| MediaError::Pipeline("pipeline has no RTP sink".into()))?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| MediaError::Pipeline("RTP sink has the wrong type".into()))?;
        Ok(Some((pipeline, sink)))
    }

    fn encoder_description(encoder: &str, bitrate_kbps: u32) -> String {
        match encoder {
            // openh264enc is the outlier: its bitrate property is bits/s.
            "openh264enc" => format!("openh264enc bitrate={}", bitrate_kbps.saturating_mul(1000)),
            "x264enc" => {
                format!("x264enc bitrate={bitrate_kbps} tune=zerolatency speed-preset=veryfast")
            }
            other => format!("{other} bitrate={bitrate_kbps}"),
        }
    }

    fn pipeline_description(
        kind: MediaKind,
        profile: &QualityProfile,
    ) -> Result<String, MediaError> {
        match kind {
            MediaKind::Voice | MediaKind::SystemAudio => {
                let source =
                    Self::first_available(&["pulsesrc", "wasapisrc", "alsasrc", "pipewiresrc"])
                        .ok_or_else(|| {
                            MediaError::Unavailable("no native audio source plugin".into())
                        })?;
                Ok(format!(
                    "{source} ! audioconvert ! audioresample ! opusenc bitrate={} ! rtpopuspay pt=111 mtu=1100 ! appsink name=rtp_sink sync=false max-buffers=8 drop=true",
                    profile.bitrate_kbps.saturating_mul(1000)
                ))
            }
            MediaKind::Camera | MediaKind::Screen => {
                let source = match kind {
                    MediaKind::Camera => Self::first_available(&[
                        "v4l2src",
                        "mfvideosrc",
                        "avfvideosrc",
                        "pipewiresrc",
                    ])
                    .map(str::to_owned),
                    MediaKind::Screen => {
                        let x11_session = std::env::var_os("DISPLAY").is_some()
                            && std::env::var_os("WAYLAND_DISPLAY").is_none();
                        if x11_session && Self::has("ximagesrc") {
                            Some("ximagesrc use-damage=false".into())
                        } else {
                            Self::first_available(&[
                                "d3d11screencapturesrc",
                                "pipewiresrc",
                                "avfvideosrc",
                                "ximagesrc",
                            ])
                            .map(str::to_owned)
                        }
                    }
                    _ => None,
                }
                .ok_or_else(|| MediaError::Unavailable("no native video source plugin".into()))?;
                let encoder = Self::first_available(&[
                    "nvh264enc",
                    "vulkanh264enc",
                    "openh264enc",
                    "x264enc",
                ])
                .ok_or_else(|| MediaError::Unavailable("no H.264 encoder plugin".into()))?;
                let encoder = Self::encoder_description(encoder, profile.bitrate_kbps);
                Ok(format!(
                    "{source} ! videoconvert ! videoscale ! videorate ! video/x-raw,width={},height={},framerate={}/1 ! {encoder} ! h264parse ! rtph264pay config-interval=-1 pt=96 mtu=1100 ! appsink name=rtp_sink sync=false max-buffers=3 drop=true",
                    profile.width, profile.height, profile.frames_per_second
                ))
            }
        }
    }

    fn receiver_description(kind: MediaKind) -> &'static str {
        match kind {
            MediaKind::Voice | MediaKind::SystemAudio => {
                "appsrc name=rtp_source is-live=true format=time do-timestamp=true caps=\"application/x-rtp,media=audio,encoding-name=OPUS,payload=111,clock-rate=48000\" ! rtpjitterbuffer latency=60 drop-on-latency=true ! rtpopusdepay ! opusdec ! audioconvert ! audioresample ! volume name=participant_volume ! autoaudiosink sync=false"
            }
            MediaKind::Camera | MediaKind::Screen => {
                "appsrc name=rtp_source is-live=true format=time do-timestamp=true caps=\"application/x-rtp,media=video,encoding-name=H264,payload=96,clock-rate=90000\" ! rtpjitterbuffer latency=80 drop-on-latency=true ! rtph264depay ! h264parse ! decodebin ! videoconvert ! autovideosink sync=false"
            }
        }
    }

    fn ensure_receiver(
        &self,
        source_id: DeviceId,
        kind: MediaKind,
    ) -> Result<gst_app::AppSrc, MediaError> {
        let mut receivers = self.receivers.lock().map_err(|_| MediaError::Poisoned)?;
        if let Some((_, _, _, source)) = receivers
            .iter()
            .find(|(source, active, _, _)| *source == source_id && *active == kind)
        {
            return Ok(source.clone());
        }
        let pipeline = if matches!(kind, MediaKind::Voice | MediaKind::SystemAudio) {
            if let Some(device) = self.selected_device(MediaDeviceKind::AudioOutput)? {
                let sink = device
                    .create_element(Some("playback_sink"))
                    .map_err(|error| MediaError::Pipeline(error.to_string()))?;
                let processing = gst::parse::bin_from_description(
                    "appsrc name=rtp_source is-live=true format=time do-timestamp=true caps=\"application/x-rtp,media=audio,encoding-name=OPUS,payload=111,clock-rate=48000\" ! rtpjitterbuffer latency=60 drop-on-latency=true ! rtpopusdepay ! opusdec ! audioconvert ! audioresample ! volume name=participant_volume",
                    true,
                )
                .map_err(|error| MediaError::Pipeline(error.to_string()))?;
                let pipeline = gst::Pipeline::new();
                pipeline
                    .add_many([processing.upcast_ref(), &sink])
                    .map_err(|error| MediaError::Pipeline(error.to_string()))?;
                processing
                    .link(&sink)
                    .map_err(|error| MediaError::Pipeline(error.to_string()))?;
                pipeline
            } else {
                let element = gst::parse::launch(Self::receiver_description(kind))
                    .map_err(|error| MediaError::Pipeline(error.to_string()))?;
                element.downcast::<gst::Pipeline>().map_err(|_| {
                    MediaError::Pipeline("receiver parser returned a non-pipeline".into())
                })?
            }
        } else {
            let element = gst::parse::launch(Self::receiver_description(kind))
                .map_err(|error| MediaError::Pipeline(error.to_string()))?;
            element.downcast::<gst::Pipeline>().map_err(|_| {
                MediaError::Pipeline("receiver parser returned a non-pipeline".into())
            })?
        };
        let source = pipeline
            .by_name("rtp_source")
            .ok_or_else(|| MediaError::Pipeline("receiver has no RTP source".into()))?
            .downcast::<gst_app::AppSrc>()
            .map_err(|_| MediaError::Pipeline("RTP source has the wrong type".into()))?;
        pipeline
            .set_state(gst::State::Playing)
            .map_err(|error| MediaError::Pipeline(error.to_string()))?;
        receivers.push((source_id, kind, pipeline, source.clone()));
        Ok(source)
    }
}

#[async_trait]
impl MediaEngine for GstMediaEngine {
    async fn capabilities(&self) -> Result<MediaCapabilities, MediaError> {
        let monitor = gst::DeviceMonitor::new();
        monitor.add_filter(Some("Audio/Source"), None);
        monitor.add_filter(Some("Audio/Sink"), None);
        monitor.add_filter(Some("Video/Source"), None);
        monitor
            .start()
            .map_err(|error| MediaError::Unavailable(error.to_string()))?;
        let mut seen_kinds = Vec::new();
        let devices = monitor
            .devices()
            .into_iter()
            .filter_map(|device| {
                let class = device.device_class();
                let kind = if class.contains("Audio/Source") {
                    MediaDeviceKind::AudioInput
                } else if class.contains("Audio/Sink") {
                    MediaDeviceKind::AudioOutput
                } else if class.contains("Video/Source") {
                    MediaDeviceKind::Camera
                } else {
                    return None;
                };
                let label = device.display_name().to_string();
                let id = Self::media_device_id(&device);
                let is_default = !seen_kinds.contains(&kind);
                seen_kinds.push(kind);
                Some(MediaDevice {
                    id,
                    label,
                    kind,
                    is_default,
                })
            })
            .collect();
        monitor.stop();
        let encoders = [
            "nvh264enc",
            "vulkanh264enc",
            "openh264enc",
            "x264enc",
            "vp8enc",
            "vp9enc",
            "av1enc",
        ]
        .into_iter()
        .filter(|name| Self::has(name))
        .map(str::to_owned)
        .collect();
        let decoders = ["avdec_h264", "openh264dec", "vp8dec", "vp9dec", "av1dec"]
            .into_iter()
            .filter(|name| Self::has(name))
            .map(str::to_owned)
            .collect();
        let zero_copy_backends = [
            "pipewiresrc",
            "d3d11screencapturesrc",
            "nvh264enc",
            "vulkanh264enc",
        ]
        .into_iter()
        .filter(|name| Self::has(name))
        .map(str::to_owned)
        .collect();
        Ok(MediaCapabilities {
            devices,
            encoders,
            decoders,
            zero_copy_backends,
        })
    }

    async fn select_device(
        &self,
        kind: MediaDeviceKind,
        device_id: Option<String>,
    ) -> Result<(), MediaError> {
        let mut selected = self
            .selected_devices
            .lock()
            .map_err(|_| MediaError::Poisoned)?;
        if let Some(device_id) = device_id {
            selected.insert(kind, device_id);
        } else {
            selected.remove(&kind);
        }
        Ok(())
    }

    async fn publish(&self, kind: MediaKind, profile: QualityProfile) -> Result<(), MediaError> {
        let profile = resolve_quality(
            &profile,
            VideoLimits {
                max_width: 3840,
                max_height: 2160,
                max_frames_per_second: 60,
                max_bitrate_kbps: 30_000,
            },
            0.0,
        )?;
        let (pipeline, appsink) =
            if let Some(selected) = self.selected_capture_pipeline(kind, &profile)? {
                selected
            } else {
                let description = Self::pipeline_description(kind, &profile)?;
                let element = gst::parse::launch(&description)
                    .map_err(|error| MediaError::Pipeline(error.to_string()))?;
                let pipeline = element.downcast::<gst::Pipeline>().map_err(|_| {
                    MediaError::Pipeline("pipeline parser returned a non-pipeline".into())
                })?;
                let appsink = pipeline
                    .by_name("rtp_sink")
                    .ok_or_else(|| MediaError::Pipeline("pipeline has no RTP sink".into()))?
                    .downcast::<gst_app::AppSink>()
                    .map_err(|_| MediaError::Pipeline("RTP sink has the wrong type".into()))?;
                (pipeline, appsink)
            };
        pipeline
            .set_state(gst::State::Playing)
            .map_err(|error| MediaError::Pipeline(error.to_string()))?;
        let mut pipelines = self.pipelines.lock().map_err(|_| MediaError::Poisoned)?;
        if let Some((_, existing, existing_sink)) =
            pipelines.iter_mut().find(|(active, _, _)| *active == kind)
        {
            existing.set_state(gst::State::Null).ok();
            *existing = pipeline;
            *existing_sink = appsink;
        } else {
            pipelines.push((kind, pipeline, appsink));
        }
        Ok(())
    }

    async fn unpublish(&self, kind: MediaKind) -> Result<(), MediaError> {
        let mut pipelines = self.pipelines.lock().map_err(|_| MediaError::Poisoned)?;
        if let Some(index) = pipelines.iter().position(|(active, _, _)| *active == kind) {
            let (_, pipeline, _) = pipelines.swap_remove(index);
            pipeline
                .set_state(gst::State::Null)
                .map_err(|error| MediaError::Pipeline(error.to_string()))?;
        }
        Ok(())
    }

    async fn stats(&self) -> Result<MediaStats, MediaError> {
        self.stats
            .lock()
            .map(|stats| stats.clone())
            .map_err(|_| MediaError::Poisoned)
    }

    async fn next_rtp_packet(&self, kind: MediaKind) -> Result<Option<Vec<u8>>, MediaError> {
        let appsink = {
            let pipelines = self.pipelines.lock().map_err(|_| MediaError::Poisoned)?;
            pipelines
                .iter()
                .find(|(active, _, _)| *active == kind)
                .map(|(_, _, sink)| sink.clone())
                .ok_or_else(|| MediaError::Pipeline("media kind is not being published".into()))?
        };
        tokio::task::spawn_blocking(move || {
            let Some(sample) = appsink.try_pull_sample(gst::ClockTime::from_mseconds(100)) else {
                return Ok(None);
            };
            let buffer = sample
                .buffer()
                .ok_or_else(|| MediaError::Pipeline("RTP sample has no buffer".into()))?;
            let map = buffer
                .map_readable()
                .map_err(|_| MediaError::Pipeline("RTP buffer is not readable".into()))?;
            Ok(Some(map.as_slice().to_vec()))
        })
        .await
        .map_err(|error| MediaError::Pipeline(error.to_string()))?
    }

    async fn receive_rtp_packet(
        &self,
        source_id: DeviceId,
        kind: MediaKind,
        packet: Vec<u8>,
    ) -> Result<(), MediaError> {
        let receiver = self.ensure_receiver(source_id, kind)?;
        let buffer = gst::Buffer::from_mut_slice(packet);
        receiver
            .push_buffer(buffer)
            .map_err(|error| MediaError::Pipeline(error.to_string()))?;
        Ok(())
    }

    async fn set_receive_volume(&self, source_id: DeviceId, volume: f64) -> Result<(), MediaError> {
        if !(0.0..=2.0).contains(&volume) {
            return Err(MediaError::Unsupported(
                "participant volume must be between 0 and 200 percent".into(),
            ));
        }
        let receivers = self.receivers.lock().map_err(|_| MediaError::Poisoned)?;
        for (source, kind, pipeline, _) in receivers.iter() {
            if *source != source_id || !matches!(kind, MediaKind::Voice | MediaKind::SystemAudio) {
                continue;
            }
            let element = pipeline
                .by_name("participant_volume")
                .ok_or_else(|| MediaError::Pipeline("receiver has no volume control".into()))?;
            element.set_property("volume", volume);
        }
        Ok(())
    }

    async fn stop_receiving(&self, kind: MediaKind) -> Result<(), MediaError> {
        let mut receivers = self.receivers.lock().map_err(|_| MediaError::Poisoned)?;
        while let Some(index) = receivers
            .iter()
            .position(|(_, active, _, _)| *active == kind)
        {
            let (_, _, pipeline, source) = receivers.swap_remove(index);
            source.end_of_stream().ok();
            pipeline
                .set_state(gst::State::Null)
                .map_err(|error| MediaError::Pipeline(error.to_string()))?;
        }
        Ok(())
    }
}

impl Drop for GstMediaEngine {
    fn drop(&mut self) {
        if let Ok(pipelines) = self.pipelines.get_mut() {
            for (_, pipeline, _) in pipelines.drain(..) {
                pipeline.set_state(gst::State::Null).ok();
            }
        }
        if let Ok(receivers) = self.receivers.get_mut() {
            for (_, _, pipeline, source) in receivers.drain(..) {
                source.end_of_stream().ok();
                pipeline.set_state(gst::State::Null).ok();
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("media backend is unavailable: {0}")]
    Unavailable(String),
    #[error("media configuration is unsupported: {0}")]
    Unsupported(String),
    #[error("media pipeline failed: {0}")]
    Pipeline(String),
    #[error("media state lock was poisoned")]
    Poisoned,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pptalk_protocol::{CallId, DeviceId};

    fn profile(mode: QualityMode) -> QualityProfile {
        QualityProfile {
            mode,
            width: 3840,
            height: 2160,
            frames_per_second: 120,
            bitrate_kbps: 30_000,
            codec: Some("h264".into()),
        }
    }

    #[test]
    fn manual_quality_is_never_silently_changed() {
        let requested = profile(QualityMode::Manual);
        let generous = VideoLimits {
            max_width: 3840,
            max_height: 2160,
            max_frames_per_second: 120,
            max_bitrate_kbps: 30_000,
        };
        assert_eq!(
            resolve_quality(&requested, generous, 0.4).expect("supported"),
            requested
        );
        assert!(
            resolve_quality(
                &requested,
                VideoLimits {
                    max_width: 1920,
                    ..generous
                },
                0.0
            )
            .is_err()
        );
    }

    #[test]
    fn automatic_quality_adapts_to_limits_and_loss() {
        let resolved = resolve_quality(
            &profile(QualityMode::Automatic),
            VideoLimits {
                max_width: 1920,
                max_height: 1080,
                max_frames_per_second: 60,
                max_bitrate_kbps: 8_000,
            },
            0.12,
        )
        .expect("automatic");
        assert_eq!(
            (resolved.width, resolved.height, resolved.frames_per_second),
            (1920, 1080, 60)
        );
        assert_eq!(resolved.bitrate_kbps, 3_600);
    }

    #[test]
    fn encoder_bitrate_units_and_low_latency_mode_are_explicit() {
        assert_eq!(
            GstMediaEngine::encoder_description("openh264enc", 2_500),
            "openh264enc bitrate=2500000"
        );
        assert_eq!(
            GstMediaEngine::encoder_description("nvh264enc", 2_500),
            "nvh264enc bitrate=2500"
        );
        assert!(GstMediaEngine::encoder_description("x264enc", 2_500).contains("tune=zerolatency"));
    }

    #[test]
    fn jitter_buffer_reorders_and_drops_late_packets() {
        let packet = |sequence| {
            MediaDatagram::new(
                CallId::from_bytes([1; 32]),
                DeviceId::from_bytes([2; 32]),
                MediaKind::Voice,
                sequence,
                sequence * 20_000,
                false,
                vec![u8::try_from(sequence).expect("small test sequence")],
            )
        };
        let mut buffer = JitterBuffer::new(1, 3);
        assert!(buffer.push(packet(2)).is_empty());
        assert_eq!(
            buffer
                .push(packet(1))
                .iter()
                .map(|item| item.sequence)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert!(buffer.push(packet(1)).is_empty());
        assert_eq!(buffer.dropped(), 1);
    }
}
