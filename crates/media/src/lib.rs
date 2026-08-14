//! Native `GStreamer` media discovery, strict quality policy and capture pipelines.

use std::{collections::BTreeMap, sync::Mutex};

#[cfg(target_os = "linux")]
use std::os::fd::{AsRawFd, OwnedFd};

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
    /// Capture element names the installed `GStreamer` can offer per family.
    pub audio_sources: Vec<String>,
    pub camera_sources: Vec<String>,
    pub screen_sources: Vec<String>,
    /// "x11", "wayland", "windows", "macos" or "none" when headless.
    pub display_session: String,
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

/// The graphical session a capture pipeline would attach to. Pure so the
/// routing table below stays regression tested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplaySession {
    None,
    X11,
    Wayland,
}

/// `WAYLAND_DISPLAY` wins over `DISPLAY`: a Wayland session usually exports
/// both (`XWayland`), but only the portal can capture the real desktop there.
#[must_use]
pub fn detect_display_session(has_x11: bool, has_wayland: bool) -> DisplaySession {
    if has_wayland {
        DisplaySession::Wayland
    } else if has_x11 {
        DisplaySession::X11
    } else {
        DisplaySession::None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenCapturePlan {
    /// `ximagesrc` against the X server directly.
    X11,
    /// `pipewiresrc` fed by a negotiated desktop-portal `ScreenCast` stream.
    PipeWirePortal,
    /// `d3d11screencapturesrc` on Windows.
    Windows,
}

/// Describes which screen capture plugins the installed `GStreamer` offers.
/// A flat options struct keeps the routing table below call-site friendly.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy)]
pub struct ScreenCaptureAvailability {
    pub is_windows: bool,
    pub has_ximagesrc: bool,
    pub has_pipewiresrc: bool,
    pub has_d3d11_screencapture: bool,
    pub portal_ready: bool,
}

/// Pure routing table for screen capture. Every failure mode gets a stable
/// code so the desktop can show an actionable message instead of `GStreamer`
/// jargon.
pub fn decide_screen_capture_source(
    session: DisplaySession,
    availability: ScreenCaptureAvailability,
) -> Result<ScreenCapturePlan, MediaError> {
    let ScreenCaptureAvailability {
        is_windows,
        has_ximagesrc,
        has_pipewiresrc,
        has_d3d11_screencapture,
        portal_ready,
    } = availability;
    if is_windows {
        return if has_d3d11_screencapture {
            Ok(ScreenCapturePlan::Windows)
        } else {
            Err(MediaError::NoScreenBackend(
                "d3d11screencapturesrc is missing".into(),
            ))
        };
    }
    match session {
        DisplaySession::None => Err(MediaError::NoDisplaySession),
        DisplaySession::X11 => {
            if has_ximagesrc {
                Ok(ScreenCapturePlan::X11)
            } else if has_pipewiresrc && portal_ready {
                Ok(ScreenCapturePlan::PipeWirePortal)
            } else {
                Err(MediaError::NoScreenBackend(
                    "no X11 or PipeWire screen capture plugin".into(),
                ))
            }
        }
        DisplaySession::Wayland => {
            if !has_pipewiresrc {
                Err(MediaError::NoScreenBackend(
                    "pipewiresrc is missing for Wayland screen capture".into(),
                ))
            } else if portal_ready {
                Ok(ScreenCapturePlan::PipeWirePortal)
            } else {
                Err(MediaError::PortalUnavailable)
            }
        }
    }
}

/// A native window the desktop lends us so remote and preview video renders
/// inside the `Qt` window through a `GStreamer` overlay instead of a foreign
/// top-level window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum VideoSurface {
    RemoteCamera,
    RemoteScreen,
    LocalPreview,
}

fn fake_video_source() -> bool {
    std::env::var_os("PPTALK_FAKE_VIDEO_SRC").is_some_and(|value| !value.is_empty())
}

fn fake_audio_source() -> bool {
    std::env::var_os("PPTALK_FAKE_AUDIO_SRC").is_some_and(|value| !value.is_empty())
}

/// Headless daemons (CLI, CI) must not spawn audio/video output windows.
fn headless_playback() -> bool {
    std::env::var_os("PPTALK_HEADLESS_PLAYBACK").is_some_and(|value| !value.is_empty())
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
    /// Attaches (or detaches with `None`) a native window handle to a video
    /// surface. Applied immediately when the matching pipeline is running.
    async fn set_video_window(
        &self,
        surface: VideoSurface,
        handle: Option<u64>,
    ) -> Result<(), MediaError>;
    /// Provides the `PipeWire` stream negotiated through the desktop portal so
    /// the next screen publish on Wayland can use it. The engine keeps the fd
    /// alive for as long as it may be needed.
    #[cfg(target_os = "linux")]
    async fn set_screen_portal_stream(&self, fd: OwnedFd, node: u32) -> Result<(), MediaError>;
}

struct CapturePipeline {
    kind: MediaKind,
    pipeline: gst::Pipeline,
    sink: gst_app::AppSink,
    preview_sink: Option<gst::Element>,
}

struct ReceiverPipeline {
    source_id: DeviceId,
    kind: MediaKind,
    pipeline: gst::Pipeline,
    source: gst_app::AppSrc,
    video_sink: Option<gst::Element>,
}

#[cfg(target_os = "linux")]
struct ScreenPortalStream {
    fd: OwnedFd,
    node: u32,
}

/// GStreamer-backed native capture/encode engine. The resulting RTP-ready
/// streams terminate at an `appsink`; the call transport owns packet delivery.
pub struct GstMediaEngine {
    pipelines: Mutex<Vec<CapturePipeline>>,
    receivers: Mutex<Vec<ReceiverPipeline>>,
    receive_volumes: Mutex<BTreeMap<DeviceId, f64>>,
    selected_devices: Mutex<BTreeMap<MediaDeviceKind, String>>,
    video_windows: Mutex<BTreeMap<VideoSurface, u64>>,
    #[cfg(target_os = "linux")]
    screen_portal: Mutex<Option<ScreenPortalStream>>,
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
            receive_volumes: Mutex::new(BTreeMap::new()),
            selected_devices: Mutex::new(BTreeMap::new()),
            video_windows: Mutex::new(BTreeMap::new()),
            #[cfg(target_os = "linux")]
            screen_portal: Mutex::new(None),
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

    fn environment_display_session() -> DisplaySession {
        detect_display_session(
            std::env::var_os("DISPLAY").is_some(),
            std::env::var_os("WAYLAND_DISPLAY").is_some(),
        )
    }

    fn screen_capture_availability(portal_ready: bool) -> ScreenCaptureAvailability {
        ScreenCaptureAvailability {
            is_windows: cfg!(windows),
            has_ximagesrc: Self::has("ximagesrc"),
            has_pipewiresrc: Self::has("pipewiresrc"),
            has_d3d11_screencapture: Self::has("d3d11screencapturesrc"),
            portal_ready,
        }
    }

    /// Prefers an overlay-capable native sink; falls back to `autovideosink`
    /// for plain CLI playback.
    fn video_sink_element() -> Result<&'static str, MediaError> {
        if cfg!(windows) {
            Self::first_available(&["d3d11videosink", "autovideosink"])
        } else {
            Self::first_available(&["xvimagesink", "ximagesink", "waylandsink", "autovideosink"])
        }
        .ok_or_else(|| MediaError::Unavailable("no native video sink plugin".into()))
    }

    fn media_device_id(device: &gst::Device) -> String {
        blake3::hash(format!("{}:{}", device.device_class(), device.display_name()).as_bytes())
            .to_hex()
            .to_string()
    }

    fn has_camera_device() -> Result<bool, MediaError> {
        let monitor = gst::DeviceMonitor::new();
        monitor.add_filter(Some("Video/Source"), None);
        monitor
            .start()
            .map_err(|error| MediaError::Unavailable(error.to_string()))?;
        let found = !monitor.devices().is_empty();
        monitor.stop();
        Ok(found)
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

    /// True when a portal-negotiated `PipeWire` stream is available.
    #[cfg(target_os = "linux")]
    fn portal_ready(&self) -> bool {
        self.screen_portal
            .lock()
            .is_ok_and(|portal| portal.is_some())
    }

    // Stubs below mirror the Linux signatures; the receiver exists so call
    // sites never need platform conditionals.
    #[allow(clippy::unused_self)]
    #[cfg(not(target_os = "linux"))]
    const fn portal_ready(&self) -> bool {
        false
    }

    /// Builds the `pipewiresrc` bound to the negotiated portal stream. The fd
    /// is cloned so the stored stream survives pipeline retries.
    #[cfg(target_os = "linux")]
    fn portal_source(&self) -> Result<gst::Element, MediaError> {
        let guard = self
            .screen_portal
            .lock()
            .map_err(|_| MediaError::Poisoned)?;
        let stream = guard.as_ref().ok_or(MediaError::PortalUnavailable)?;
        let fd = stream
            .fd
            .try_clone()
            .map_err(|error| MediaError::PortalFailed(error.to_string()))?;
        gst::ElementFactory::make("pipewiresrc")
            .name("portal_screen_source")
            .property("fd", i64::from(fd.as_raw_fd()))
            .property("path", stream.node.to_string())
            .property("always-zero-timestamps", true)
            .build()
            .map_err(|error| MediaError::Pipeline(error.to_string()))
    }

    /// The portal path only exists on Linux; the routing table never returns
    /// `PipeWirePortal` elsewhere, so this arm is unreachable in practice.
    #[allow(clippy::unused_self)]
    #[cfg(not(target_os = "linux"))]
    fn portal_source(&self) -> Result<gst::Element, MediaError> {
        Err(MediaError::PortalUnavailable)
    }

    /// The encode chain shared by camera and screen. `preview_tail` appends a
    /// small raw preview branch behind a tee when the desktop attached a local
    /// preview window.
    fn encode_chain(profile: &QualityProfile, preview_tail: bool) -> Result<String, MediaError> {
        let encoder =
            Self::first_available(&["nvh264enc", "vulkanh264enc", "openh264enc", "x264enc"])
                .ok_or_else(|| MediaError::Unavailable("no H.264 encoder plugin".into()))?;
        let encoder = Self::encoder_description(encoder, profile.bitrate_kbps);
        let encode = format!(
            "videoconvert ! videoscale ! videorate ! video/x-raw,width={},height={},framerate={}/1 ! {encoder} ! h264parse ! rtph264pay config-interval=-1 pt=96 mtu=1100 ! appsink name=rtp_sink sync=false max-buffers=3 drop=true",
            profile.width, profile.height, profile.frames_per_second
        );
        if !preview_tail {
            return Ok(encode);
        }
        let preview_sink = Self::video_sink_element()?;
        Ok(format!(
            "tee name=capture_tee ! queue ! {encode} capture_tee. ! queue leaky=downstream max-size-buffers=2 ! videoscale ! video/x-raw,width=480 ! videoconvert ! {preview_sink} name=preview_sink sync=false qos=false"
        ))
    }

    /// Builds the capture pipeline for a kind, applying fake sources for CI
    /// and the routing table for screen capture.
    fn build_capture(
        &self,
        kind: MediaKind,
        profile: &QualityProfile,
    ) -> Result<(gst::Pipeline, gst_app::AppSink, Option<gst::Element>), MediaError> {
        let preview_tail = matches!(kind, MediaKind::Camera | MediaKind::Screen)
            && self
                .video_windows
                .lock()
                .map_err(|_| MediaError::Poisoned)?
                .contains_key(&VideoSurface::LocalPreview);
        match kind {
            MediaKind::Voice | MediaKind::SystemAudio => {
                let source = if fake_audio_source() {
                    "audiotestsrc is-live=true".to_owned()
                } else {
                    Self::first_available(&["pulsesrc", "wasapisrc", "alsasrc", "pipewiresrc"])
                        .ok_or_else(|| {
                            MediaError::Unavailable("no native audio source plugin".into())
                        })?
                        .to_owned()
                };
                let description = format!(
                    "{source} ! audioconvert ! audioresample ! opusenc bitrate={} ! rtpopuspay pt=111 mtu=1100 ! appsink name=rtp_sink sync=false max-buffers=8 drop=true",
                    profile.bitrate_kbps.saturating_mul(1000)
                );
                Self::launch_capture(&description, false)
            }
            MediaKind::Camera => {
                if fake_video_source() {
                    let description = format!(
                        "videotestsrc is-live=true pattern=ball ! {}",
                        Self::encode_chain(profile, preview_tail)?
                    );
                    return Self::launch_capture(&description, preview_tail);
                }
                if let Some(device) = self.selected_device(MediaDeviceKind::Camera)? {
                    let source = device
                        .create_element(Some("capture_source"))
                        .map_err(|error| MediaError::Pipeline(error.to_string()))?;
                    let remainder = Self::encode_chain(profile, preview_tail)?;
                    return Self::pipeline_from_source(&source, &remainder, preview_tail);
                }
                let source =
                    Self::first_available(&["v4l2src", "mfvideosrc", "avfvideosrc", "pipewiresrc"])
                        .ok_or_else(|| {
                            MediaError::Unavailable("no native video source plugin".into())
                        })?;
                let description =
                    format!("{source} ! {}", Self::encode_chain(profile, preview_tail)?);
                Self::launch_capture(&description, preview_tail)
            }
            MediaKind::Screen => {
                if fake_video_source() {
                    let description = format!(
                        "videotestsrc is-live=true pattern=ball ! {}",
                        Self::encode_chain(profile, preview_tail)?
                    );
                    return Self::launch_capture(&description, preview_tail);
                }
                let session = Self::environment_display_session();
                let plan = decide_screen_capture_source(
                    session,
                    Self::screen_capture_availability(self.portal_ready()),
                )?;
                let chain = Self::encode_chain(profile, preview_tail)?;
                match plan {
                    ScreenCapturePlan::X11 => {
                        let description = format!("ximagesrc use-damage=false ! {chain}");
                        Self::launch_capture(&description, preview_tail)
                    }
                    ScreenCapturePlan::Windows => {
                        let Some(source) =
                            Self::first_available(&["d3d11screencapturesrc", "ximagesrc"])
                        else {
                            return Err(MediaError::NoScreenBackend(
                                "no native screen capture plugin".into(),
                            ));
                        };
                        let description = format!("{source} ! {chain}");
                        Self::launch_capture(&description, preview_tail)
                    }
                    ScreenCapturePlan::PipeWirePortal => {
                        let source = self.portal_source()?;
                        Self::pipeline_from_source(&source, &chain, preview_tail)
                    }
                }
            }
        }
    }

    fn launch_capture(
        description: &str,
        with_preview: bool,
    ) -> Result<(gst::Pipeline, gst_app::AppSink, Option<gst::Element>), MediaError> {
        let element = gst::parse::launch(description)
            .map_err(|error| MediaError::Pipeline(error.to_string()))?;
        let pipeline = element
            .downcast::<gst::Pipeline>()
            .map_err(|_| MediaError::Pipeline("pipeline parser returned a non-pipeline".into()))?;
        Self::capture_handles(&pipeline, with_preview)
    }

    fn pipeline_from_source(
        source: &gst::Element,
        remainder: &str,
        with_preview: bool,
    ) -> Result<(gst::Pipeline, gst_app::AppSink, Option<gst::Element>), MediaError> {
        let processing = gst::parse::bin_from_description(remainder, true)
            .map_err(|error| MediaError::Pipeline(error.to_string()))?;
        let pipeline = gst::Pipeline::new();
        pipeline
            .add_many([source, processing.upcast_ref()])
            .map_err(|error| MediaError::Pipeline(error.to_string()))?;
        source
            .link(&processing)
            .map_err(|error| MediaError::Pipeline(error.to_string()))?;
        Self::capture_handles(&pipeline, with_preview)
    }

    fn capture_handles(
        pipeline: &gst::Pipeline,
        with_preview: bool,
    ) -> Result<(gst::Pipeline, gst_app::AppSink, Option<gst::Element>), MediaError> {
        let sink = pipeline
            .by_name("rtp_sink")
            .ok_or_else(|| MediaError::Pipeline("pipeline has no RTP sink".into()))?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| MediaError::Pipeline("RTP sink has the wrong type".into()))?;
        let preview_sink = if with_preview {
            pipeline
                .by_name("preview_sink")
                .map(Some)
                .ok_or_else(|| MediaError::Pipeline("pipeline has no preview sink".into()))?
        } else {
            None
        };
        Ok((pipeline.clone(), sink, preview_sink))
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

    fn receiver_description(kind: MediaKind, sink_element: &str) -> String {
        match kind {
            MediaKind::Voice | MediaKind::SystemAudio => format!(
                "appsrc name=rtp_source is-live=true format=time do-timestamp=true caps=\"application/x-rtp,media=audio,encoding-name=OPUS,payload=111,clock-rate=48000\" ! rtpjitterbuffer latency=60 drop-on-latency=true ! rtpopusdepay ! opusdec ! audioconvert ! audioresample ! volume name=participant_volume ! {sink_element} sync=false"
            ),
            MediaKind::Camera | MediaKind::Screen => format!(
                "appsrc name=rtp_source is-live=true format=time do-timestamp=true caps=\"application/x-rtp,media=video,encoding-name=H264,payload=96,clock-rate=90000\" ! rtpjitterbuffer latency=80 drop-on-latency=true ! rtph264depay ! h264parse ! decodebin ! videoconvert ! {sink_element} name=video_sink sync=false"
            ),
        }
    }

    /// Overlay application is audited FFI: the handle comes from the `Qt`
    /// window we own and `GStreamer` only renders into it.
    #[allow(unsafe_code)]
    fn apply_window_handle(element: &gst::Element, handle: u64) -> bool {
        use gst_video::prelude::*;
        use gstreamer_video as gst_video;
        let Some(overlay) = element.dynamic_cast_ref::<gst_video::VideoOverlay>() else {
            return false;
        };
        // SAFETY: gst_video_overlay_set_window_handle only stores the handle
        // for the sink's own window creation; it dereferences nothing.
        unsafe {
            VideoOverlayExtManual::set_window_handle(
                overlay,
                usize::try_from(handle).unwrap_or(usize::MAX),
            );
        }
        overlay.expose();
        true
    }

    fn surface_for(kind: MediaKind) -> Option<VideoSurface> {
        match kind {
            MediaKind::Camera => Some(VideoSurface::RemoteCamera),
            MediaKind::Screen => Some(VideoSurface::RemoteScreen),
            MediaKind::Voice | MediaKind::SystemAudio => None,
        }
    }

    fn ensure_receiver(
        &self,
        source_id: DeviceId,
        kind: MediaKind,
    ) -> Result<gst_app::AppSrc, MediaError> {
        let receivers = self.receivers.lock().map_err(|_| MediaError::Poisoned)?;
        if let Some(receiver) = receivers
            .iter()
            .find(|active| active.source_id == source_id && active.kind == kind)
        {
            return Ok(receiver.source.clone());
        }
        let audio = matches!(kind, MediaKind::Voice | MediaKind::SystemAudio);
        let headless = headless_playback();
        let handle_present = !audio
            && !headless
            && Self::surface_for(kind).is_some_and(|surface| {
                self.video_windows
                    .lock()
                    .map_err(|_| MediaError::Poisoned)
                    .is_ok_and(|windows| windows.contains_key(&surface))
            });
        let sink_element = if headless {
            "fakesink".to_owned()
        } else if audio {
            "autoaudiosink".to_owned()
        } else if handle_present {
            Self::video_sink_element()?.to_owned()
        } else {
            "autovideosink".to_owned()
        };
        let description = Self::receiver_description(kind, &sink_element);
        let element = gst::parse::launch(&description)
            .map_err(|error| MediaError::Pipeline(error.to_string()))?;
        let pipeline = element
            .downcast::<gst::Pipeline>()
            .map_err(|_| MediaError::Pipeline("receiver parser returned a non-pipeline".into()))?;
        Self::finish_receiver(
            self,
            receivers,
            pipeline,
            source_id,
            kind,
            !audio && !headless && handle_present,
        )
    }

    fn finish_receiver(
        engine: &Self,
        mut receivers: std::sync::MutexGuard<'_, Vec<ReceiverPipeline>>,
        pipeline: gst::Pipeline,
        source_id: DeviceId,
        kind: MediaKind,
        with_video_sink: bool,
    ) -> Result<gst_app::AppSrc, MediaError> {
        let source = pipeline
            .by_name("rtp_source")
            .ok_or_else(|| MediaError::Pipeline("receiver has no RTP source".into()))?
            .downcast::<gst_app::AppSrc>()
            .map_err(|_| MediaError::Pipeline("RTP source has the wrong type".into()))?;
        if matches!(kind, MediaKind::Voice | MediaKind::SystemAudio) {
            let volume = engine
                .receive_volumes
                .lock()
                .map_err(|_| MediaError::Poisoned)?
                .get(&source_id)
                .copied()
                .unwrap_or(1.0);
            let element = pipeline
                .by_name("participant_volume")
                .ok_or_else(|| MediaError::Pipeline("receiver has no volume control".into()))?;
            element.set_property("volume", volume);
        }
        let video_sink = with_video_sink
            .then(|| pipeline.by_name("video_sink"))
            .flatten();
        pipeline
            .set_state(gst::State::Playing)
            .map_err(|error| MediaError::Pipeline(error.to_string()))?;
        if let (Some(sink), Some(surface)) = (
            video_sink.as_ref(),
            Self::surface_for(kind).filter(|_| !headless_playback()),
        ) && let Some(&handle) = engine
            .video_windows
            .lock()
            .map_err(|_| MediaError::Poisoned)?
            .get(&surface)
        {
            Self::apply_window_handle(sink, handle);
        }
        receivers.push(ReceiverPipeline {
            source_id,
            kind,
            pipeline,
            source: source.clone(),
            video_sink,
        });
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
        let present = |names: &[&str]| {
            names
                .iter()
                .filter(|name| Self::has(name))
                .map(|name| (*name).to_owned())
                .collect::<Vec<_>>()
        };
        let audio_sources = present(&["pulsesrc", "wasapisrc", "alsasrc", "pipewiresrc"]);
        let camera_sources = present(&["v4l2src", "mfvideosrc", "avfvideosrc", "pipewiresrc"]);
        let screen_sources = present(&[
            "ximagesrc",
            "pipewiresrc",
            "d3d11screencapturesrc",
            "avfvideosrc",
        ]);
        let display_session = if cfg!(windows) {
            "windows".to_owned()
        } else if cfg!(target_os = "macos") {
            "macos".to_owned()
        } else {
            match Self::environment_display_session() {
                DisplaySession::X11 => "x11",
                DisplaySession::Wayland => "wayland",
                DisplaySession::None => "none",
            }
            .to_owned()
        };
        Ok(MediaCapabilities {
            devices,
            encoders,
            decoders,
            zero_copy_backends,
            audio_sources,
            camera_sources,
            screen_sources,
            display_session,
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
        let synthetic_source = match kind {
            MediaKind::Camera | MediaKind::Screen => fake_video_source(),
            MediaKind::Voice | MediaKind::SystemAudio => fake_audio_source(),
        };
        if !synthetic_source {
            match kind {
                MediaKind::Camera => {
                    if !Self::has_camera_device()? {
                        return Err(MediaError::NoCameraDevice);
                    }
                }
                MediaKind::Screen => {
                    decide_screen_capture_source(
                        Self::environment_display_session(),
                        Self::screen_capture_availability(self.portal_ready()),
                    )?;
                }
                MediaKind::Voice | MediaKind::SystemAudio => {}
            }
        }
        let (pipeline, appsink, preview_sink) = self.build_capture(kind, &profile)?;
        pipeline
            .set_state(gst::State::Playing)
            .map_err(|error| MediaError::Pipeline(error.to_string()))?;
        if let Some(sink) = &preview_sink
            && let Some(&handle) = self
                .video_windows
                .lock()
                .map_err(|_| MediaError::Poisoned)?
                .get(&VideoSurface::LocalPreview)
        {
            Self::apply_window_handle(sink, handle);
        }
        let mut pipelines = self.pipelines.lock().map_err(|_| MediaError::Poisoned)?;
        if let Some(existing) = pipelines.iter_mut().find(|active| active.kind == kind) {
            existing.pipeline.set_state(gst::State::Null).ok();
            existing.pipeline = pipeline;
            existing.sink = appsink;
            existing.preview_sink = preview_sink;
        } else {
            pipelines.push(CapturePipeline {
                kind,
                pipeline,
                sink: appsink,
                preview_sink,
            });
        }
        Ok(())
    }

    async fn unpublish(&self, kind: MediaKind) -> Result<(), MediaError> {
        let mut pipelines = self.pipelines.lock().map_err(|_| MediaError::Poisoned)?;
        if let Some(index) = pipelines.iter().position(|active| active.kind == kind) {
            let removed = pipelines.swap_remove(index);
            removed
                .pipeline
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
                .find(|active| active.kind == kind)
                .map(|active| active.sink.clone())
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
        self.receive_volumes
            .lock()
            .map_err(|_| MediaError::Poisoned)?
            .insert(source_id, volume);
        let receivers = self.receivers.lock().map_err(|_| MediaError::Poisoned)?;
        for receiver in receivers.iter() {
            if receiver.source_id != source_id
                || !matches!(receiver.kind, MediaKind::Voice | MediaKind::SystemAudio)
            {
                continue;
            }
            let element = receiver
                .pipeline
                .by_name("participant_volume")
                .ok_or_else(|| MediaError::Pipeline("receiver has no volume control".into()))?;
            element.set_property("volume", volume);
        }
        Ok(())
    }

    async fn stop_receiving(&self, kind: MediaKind) -> Result<(), MediaError> {
        let mut receivers = self.receivers.lock().map_err(|_| MediaError::Poisoned)?;
        while let Some(index) = receivers.iter().position(|active| active.kind == kind) {
            let removed = receivers.swap_remove(index);
            removed.source.end_of_stream().ok();
            removed
                .pipeline
                .set_state(gst::State::Null)
                .map_err(|error| MediaError::Pipeline(error.to_string()))?;
        }
        Ok(())
    }

    async fn set_video_window(
        &self,
        surface: VideoSurface,
        handle: Option<u64>,
    ) -> Result<(), MediaError> {
        let mut windows = self
            .video_windows
            .lock()
            .map_err(|_| MediaError::Poisoned)?;
        match handle {
            Some(handle) => windows.insert(surface, handle),
            None => windows.remove(&surface),
        };
        drop(windows);
        if headless_playback() {
            return Ok(());
        }
        match surface {
            VideoSurface::LocalPreview => {
                let pipelines = self.pipelines.lock().map_err(|_| MediaError::Poisoned)?;
                for pipeline in pipelines.iter() {
                    let Some(sink) = &pipeline.preview_sink else {
                        continue;
                    };
                    if let Some(&handle) = self
                        .video_windows
                        .lock()
                        .map_err(|_| MediaError::Poisoned)?
                        .get(&VideoSurface::LocalPreview)
                    {
                        Self::apply_window_handle(sink, handle);
                    }
                }
            }
            surface @ (VideoSurface::RemoteCamera | VideoSurface::RemoteScreen) => {
                let kind = match surface {
                    VideoSurface::RemoteCamera => MediaKind::Camera,
                    VideoSurface::RemoteScreen => MediaKind::Screen,
                    VideoSurface::LocalPreview => unreachable!(),
                };
                let receivers = self.receivers.lock().map_err(|_| MediaError::Poisoned)?;
                let handle = self
                    .video_windows
                    .lock()
                    .map_err(|_| MediaError::Poisoned)?
                    .get(&surface)
                    .copied();
                for receiver in receivers.iter().filter(|active| active.kind == kind) {
                    if let Some(sink) = &receiver.video_sink
                        && let Some(handle) = handle
                    {
                        Self::apply_window_handle(sink, handle);
                    }
                }
            }
        }
        Ok(())
    }

    #[cfg(target_os = "linux")]
    async fn set_screen_portal_stream(&self, fd: OwnedFd, node: u32) -> Result<(), MediaError> {
        *self
            .screen_portal
            .lock()
            .map_err(|_| MediaError::Poisoned)? = Some(ScreenPortalStream { fd, node });
        Ok(())
    }
}

impl Drop for GstMediaEngine {
    fn drop(&mut self) {
        if let Ok(pipelines) = self.pipelines.get_mut() {
            for removed in pipelines.drain(..) {
                removed.pipeline.set_state(gst::State::Null).ok();
            }
        }
        if let Ok(receivers) = self.receivers.get_mut() {
            for removed in receivers.drain(..) {
                removed.source.end_of_stream().ok();
                removed.pipeline.set_state(gst::State::Null).ok();
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
    #[error("no camera is connected to this device")]
    NoCameraDevice,
    #[error("there is no graphical session to capture")]
    NoDisplaySession,
    #[error("no screen capture backend is available: {0}")]
    NoScreenBackend(String),
    #[error("the desktop portal denied the screen cast session")]
    PortalDenied(String),
    #[error("the desktop portal session failed: {0}")]
    PortalFailed(String),
    #[error("screen sharing needs permission through the desktop portal first")]
    PortalUnavailable,
}

impl MediaError {
    /// Stable machine-readable code for the desktop UI. Generic pipeline and
    /// availability failures are classified by the caller using the media
    /// kind, because only that context distinguishes them.
    #[must_use]
    pub fn code(&self) -> Option<&'static str> {
        match self {
            Self::Unavailable(_) | Self::Unsupported(_) | Self::Pipeline(_) | Self::Poisoned => {
                None
            }
            Self::NoCameraDevice => Some("no_camera_device"),
            Self::NoDisplaySession => Some("no_display_session"),
            Self::NoScreenBackend(_) => Some("no_screen_backend"),
            Self::PortalDenied(_) => Some("portal_denied"),
            Self::PortalFailed(_) => Some("portal_failed"),
            Self::PortalUnavailable => Some("portal_unavailable"),
        }
    }
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

    #[test]
    fn display_session_prefers_wayland_over_xwayland() {
        assert_eq!(detect_display_session(true, false), DisplaySession::X11);
        assert_eq!(detect_display_session(false, true), DisplaySession::Wayland);
        assert_eq!(detect_display_session(true, true), DisplaySession::Wayland);
        assert_eq!(detect_display_session(false, false), DisplaySession::None);
    }

    #[test]
    fn screen_capture_routing_covers_every_failure_mode() {
        let availability = |is_windows, x11, pipewire, d3d11, portal| ScreenCaptureAvailability {
            is_windows,
            has_ximagesrc: x11,
            has_pipewiresrc: pipewire,
            has_d3d11_screencapture: d3d11,
            portal_ready: portal,
        };
        // Headless Linux never reaches a pipeline: it fails with a code.
        assert!(matches!(
            decide_screen_capture_source(
                DisplaySession::None,
                availability(false, true, true, false, false)
            ),
            Err(MediaError::NoDisplaySession)
        ));
        // X11 with the plugin captures directly.
        assert_eq!(
            decide_screen_capture_source(
                DisplaySession::X11,
                availability(false, true, true, false, false)
            )
            .expect("x11"),
            ScreenCapturePlan::X11
        );
        // X11 without ximagesrc can still go through a negotiated portal.
        assert_eq!(
            decide_screen_capture_source(
                DisplaySession::X11,
                availability(false, false, true, false, true)
            )
            .expect("portal over x11"),
            ScreenCapturePlan::PipeWirePortal
        );
        // X11 with nothing reports the missing backend.
        assert!(matches!(
            decide_screen_capture_source(
                DisplaySession::X11,
                availability(false, false, false, false, true)
            ),
            Err(MediaError::NoScreenBackend(_))
        ));
        // Wayland always goes through the portal.
        assert_eq!(
            decide_screen_capture_source(
                DisplaySession::Wayland,
                availability(false, true, true, false, true)
            )
            .expect("wayland portal"),
            ScreenCapturePlan::PipeWirePortal
        );
        // Wayland without a negotiated stream asks for permission first.
        assert!(matches!(
            decide_screen_capture_source(
                DisplaySession::Wayland,
                availability(false, true, true, false, false)
            ),
            Err(MediaError::PortalUnavailable)
        ));
        // Wayland without pipewiresrc reports the missing backend.
        assert!(matches!(
            decide_screen_capture_source(
                DisplaySession::Wayland,
                availability(false, true, false, false, true)
            ),
            Err(MediaError::NoScreenBackend(_))
        ));
        // Windows uses d3d11 and never consults the session.
        assert_eq!(
            decide_screen_capture_source(
                DisplaySession::None,
                availability(true, false, false, true, false)
            )
            .expect("windows"),
            ScreenCapturePlan::Windows
        );
        assert!(matches!(
            decide_screen_capture_source(
                DisplaySession::None,
                availability(true, false, false, false, false)
            ),
            Err(MediaError::NoScreenBackend(_))
        ));
    }

    #[test]
    fn media_error_codes_are_stable_for_the_desktop() {
        assert_eq!(MediaError::NoCameraDevice.code(), Some("no_camera_device"));
        assert_eq!(
            MediaError::NoDisplaySession.code(),
            Some("no_display_session")
        );
        assert_eq!(
            MediaError::NoScreenBackend(String::new()).code(),
            Some("no_screen_backend")
        );
        assert_eq!(
            MediaError::PortalDenied(String::new()).code(),
            Some("portal_denied")
        );
        assert_eq!(
            MediaError::PortalFailed(String::new()).code(),
            Some("portal_failed")
        );
        assert_eq!(
            MediaError::PortalUnavailable.code(),
            Some("portal_unavailable")
        );
        assert_eq!(MediaError::Pipeline(String::new()).code(), None);
    }

    #[tokio::test]
    async fn participant_volume_is_remembered_before_media_arrives() {
        let engine = GstMediaEngine::new().expect("GStreamer");
        let device = DeviceId::from_bytes([7; 32]);

        engine
            .set_receive_volume(device, 0.35)
            .await
            .expect("store participant volume");

        assert_eq!(
            engine
                .receive_volumes
                .lock()
                .expect("volume settings")
                .get(&device)
                .copied(),
            Some(0.35)
        );
    }

    #[test]
    fn opus_rtp_voice_roundtrips_without_audio_hardware() {
        gst::init().expect("GStreamer");
        let missing = [
            "audiotestsrc",
            "opusenc",
            "rtpopuspay",
            "appsrc",
            "rtpjitterbuffer",
            "rtpopusdepay",
            "opusdec",
            "appsink",
        ]
        .into_iter()
        .filter(|element| !GstMediaEngine::has(element))
        .collect::<Vec<_>>();
        if !missing.is_empty() {
            eprintln!(
                "skipping synthetic voice roundtrip; missing GStreamer elements: {missing:?}"
            );
            return;
        }

        let capture = gst::parse::launch(
            "audiotestsrc num-buffers=30 samplesperbuffer=960 wave=sine freq=440 \
             ! audio/x-raw,format=S16LE,rate=48000,channels=1 \
             ! opusenc bitrate=64000 ! rtpopuspay pt=111 mtu=1100 \
             ! appsink name=encoded sync=false",
        )
        .expect("synthetic capture")
        .downcast::<gst::Pipeline>()
        .expect("capture pipeline");
        let encoded = capture
            .by_name("encoded")
            .expect("encoded sink")
            .downcast::<gst_app::AppSink>()
            .expect("appsink");

        let playback = gst::parse::launch(
            "appsrc name=rtp_source is-live=true format=time do-timestamp=true \
             caps=\"application/x-rtp,media=audio,encoding-name=OPUS,payload=111,clock-rate=48000\" \
             ! rtpjitterbuffer latency=20 ! rtpopusdepay ! opusdec \
             ! audioconvert ! audio/x-raw,format=S16LE,rate=48000,channels=1 \
             ! appsink name=decoded sync=false",
        )
        .expect("synthetic playback")
        .downcast::<gst::Pipeline>()
        .expect("playback pipeline");
        let source = playback
            .by_name("rtp_source")
            .expect("RTP source")
            .downcast::<gst_app::AppSrc>()
            .expect("appsrc");
        let decoded = playback
            .by_name("decoded")
            .expect("decoded sink")
            .downcast::<gst_app::AppSink>()
            .expect("appsink");

        playback.set_state(gst::State::Playing).expect("playback");
        capture.set_state(gst::State::Playing).expect("capture");
        for _ in 0..30 {
            let sample = encoded
                .try_pull_sample(gst::ClockTime::from_seconds(2))
                .expect("encoded RTP packet");
            let buffer = sample.buffer().expect("encoded buffer");
            let bytes = buffer.map_readable().expect("readable RTP");
            source
                .push_buffer(gst::Buffer::from_slice(bytes.as_slice().to_vec()))
                .expect("push RTP");
        }
        source.end_of_stream().expect("RTP end");

        let sample = decoded
            .try_pull_sample(gst::ClockTime::from_seconds(2))
            .expect("decoded voice");
        let buffer = sample.buffer().expect("decoded buffer");
        let audio = buffer.map_readable().expect("readable PCM");
        assert!(
            audio.as_slice().iter().any(|sample| *sample != 0),
            "decoded voice must not be silent"
        );

        capture.set_state(gst::State::Null).expect("stop capture");
        playback.set_state(gst::State::Null).expect("stop playback");
    }
}
