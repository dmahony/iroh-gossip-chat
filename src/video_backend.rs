//! Shared inline-video pipeline construction.
//!
//! Keep libav-owned decode buffers separate from downstream NV12 buffers.
//! With direct rendering, cropped H.264 frames (406 pixels wide, padded to
//! 416) can trigger a fatal GLib video-meta assertion during allocation.
//! This is a native abort, not a recoverable player/bus error.

use gst::prelude::*;
use gstreamer as gst;
use gstreamer_app::AppSink;
use iced_video_player::{Error, Video};

fn configure_decoder(element: &gst::Element) {
    if element
        .factory()
        .is_some_and(|factory| factory.name().starts_with("avdec_"))
        && element.find_property("direct-rendering").is_some()
    {
        element.set_property("direct-rendering", false);
    }
}

fn build_pipeline(uri: &url::Url) -> Result<(gst::Pipeline, AppSink, AppSink), Error> {
    gst::init()?;
    // Set the URI as a property, never interpolate it into pipeline syntax.
    // Retain playbin so seeking, volume, subtitles and stream URLs keep the
    // same semantics as iced_video_player::Video::new.
    let pipeline = gst::ElementFactory::make("playbin")
        .property("uri", uri.as_str())
        .build()?
        .downcast::<gst::Pipeline>()
        .map_err(|_| Error::Cast)?;
    let sink_bin = gst::parse::bin_from_description(
        "videoscale ! videoconvert ! appsink name=iced_video drop=true max-buffers=1 caps=video/x-raw,format=NV12,pixel-aspect-ratio=1/1",
        true,
    )?;
    let video_sink = sink_bin
        .by_name("iced_video")
        .ok_or_else(|| Error::AppSink("iced_video".into()))?
        .downcast::<AppSink>()
        .map_err(|_| Error::Cast)?;
    let text_sink = gst::ElementFactory::make("appsink")
        .name("iced_text")
        .property("sync", true)
        .property("drop", true)
        .build()?
        .downcast::<AppSink>()
        .map_err(|_| Error::Cast)?;
    pipeline.set_property("video-sink", &sink_bin);
    pipeline.set_property("text-sink", &text_sink);
    // Decodebin creates codec elements lazily. Configure them synchronously
    // as they join the pipeline, before the first allocation/state change.
    // This also covers replacement decoders after stream renegotiation.
    pipeline.connect_deep_element_added(|_, _, element| configure_decoder(element));
    Ok((pipeline, video_sink, text_sink))
}

/// Open an already-authorized local attachment or Boru loopback stream.
///
/// Callers must retain their existing content-identity/stream authorization
/// checks. Only decoder allocation changes here, not attachment admission.
pub fn open_video(uri: &url::Url) -> Result<Video, Error> {
    let (pipeline, video_sink, text_sink) = build_pipeline(uri)?;
    Video::from_gst_pipeline(pipeline, video_sink, Some(text_sink))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dynamically_added_libav_decoder_disables_direct_rendering() {
        let uri = url::Url::parse("file:///unused.mp4").unwrap();
        let (pipeline, _, _) = build_pipeline(&uri).unwrap();
        let decodebin = gst::Bin::new();
        pipeline.add(&decodebin).unwrap();
        let decoder = gst::ElementFactory::make("avdec_h264").build().unwrap();
        assert!(decoder.property::<bool>("direct-rendering"));
        decodebin.add(&decoder).unwrap();
        assert!(!decoder.property::<bool>("direct-rendering"));
        // Unrelated elements need not expose this property.
        configure_decoder(&gst::ElementFactory::make("fakesink").build().unwrap());
    }

    /// Run explicitly on the affected runtime with the original fixture.
    /// Accelerate sinks only; use the production pipeline and Video worker.
    #[test]
    #[ignore = "requires BORU_VIDEO_REGRESSION_FILE and GStreamer codec runtime"]
    fn cropped_h264_reaches_eos_without_native_abort() {
        let path = std::path::PathBuf::from(
            std::env::var_os("BORU_VIDEO_REGRESSION_FILE").expect("set fixture path"),
        );
        let uri = url::Url::from_file_path(path.canonicalize().unwrap()).unwrap();
        for _ in 0..8 {
            let (pipeline, video_sink, text_sink) = build_pipeline(&uri).unwrap();
            let audio_sink = gst::ElementFactory::make("fakesink")
                .property("sync", false)
                .build()
                .unwrap();
            pipeline.set_property("audio-sink", &audio_sink);
            video_sink.set_sync(false);
            text_sink.set_sync(false);
            let bus = pipeline.bus().unwrap();
            let video = Video::from_gst_pipeline(pipeline, video_sink, Some(text_sink)).unwrap();
            let message = bus
                .timed_pop_filtered(
                    gst::ClockTime::from_seconds(60),
                    &[gst::MessageType::Eos, gst::MessageType::Error],
                )
                .expect("playback did not reach EOS within 60 seconds");
            if let gst::MessageView::Error(error) = message.view() {
                panic!("decoder error: {} ({:?})", error.error(), error.debug());
            }
            assert!(matches!(message.view(), gst::MessageView::Eos(_)));
            drop(video);
        }
    }
}
