use cairo::{Context, Format, ImageSurface};
use druid::{Affine, RenderContext};
use gst::prelude::*;
use gstreamer as gst;
use gstreamer_app as gst_app;
use gstreamer_audio as gst_audio;
use gstreamer_video as gst_video;
use piet_cairo::CairoRenderContext;
use std::path::Path;

use crate::audio::{AudioSnippetsData, Cursor, SAMPLE_RATE};
use crate::data::SnippetsData;
use crate::time::{self, Time};

const FPS: f64 = 30.0;
const WIDTH: i32 = 800;
const HEIGHT: i32 = 600;

// We make a custom error here because the default display for gst::message::Error isn't very
// helpful in narrowing down the problem.
#[derive(Debug, thiserror::Error)]
#[error("error from {src}: {error} ({debug})")]
struct PipelineError {
    src: String,
    error: String,
    debug: String,
}

impl<'a> From<gst::message::Error<'a>> for PipelineError {
    fn from(e: gst::message::Error<'a>) -> PipelineError {
        PipelineError {
            src: e
                .get_src()
                .map(|s| String::from(s.get_path_string()))
                .unwrap_or_else(|| "None".to_owned()),
            error: e.get_error().to_string(),
            debug: e.get_debug().unwrap_or_else(|| "No debug info".to_owned()),
        }
    }
}

fn create_pipeline(
    anim: SnippetsData,
    audio: AudioSnippetsData,
    frame_count: u32,
    path: &Path,
) -> Result<gst::Pipeline, anyhow::Error> {
    let pipeline = gst::Pipeline::new(None);
    let v_src = gst::ElementFactory::make("appsrc", Some("source"))?;
    let v_convert = gst::ElementFactory::make("videoconvert", Some("convert"))?;
    let v_encode = gst::ElementFactory::make("vp9enc", Some("encode"))?;
    let v_queue1 = gst::ElementFactory::make("queue", Some("queue1"))?;
    let v_queue2 = gst::ElementFactory::make("queue", Some("queue2"))?;
    let a_src = gst::ElementFactory::make("appsrc", Some("audio-source"))?;
    let a_convert = gst::ElementFactory::make("audioconvert", Some("audio-convert"))?;
    let a_encode = gst::ElementFactory::make("vorbisenc", Some("audio-encode"))?;
    let a_queue1 = gst::ElementFactory::make("queue", Some("audio-queue1"))?;
    let a_queue2 = gst::ElementFactory::make("queue", Some("audio-queue2"))?;
    let mux = gst::ElementFactory::make("webmmux", Some("mux"))?;
    let sink = gst::ElementFactory::make("filesink", Some("sink"))?;

    pipeline.add_many(&[&v_src, &v_convert, &v_encode, &v_queue1, &v_queue2])?;
    pipeline.add_many(&[&a_src, &a_convert, &a_encode, &a_queue1, &a_queue2])?;
    pipeline.add_many(&[&mux, &sink])?;
    gst::Element::link_many(&[&v_src, &v_queue1, &v_convert, &v_encode, &v_queue2, &mux])?;
    gst::Element::link_many(&[&a_src, &a_queue1, &a_convert, &a_encode, &a_queue2, &mux])?;
    gst::Element::link(&mux, &sink)?;

    // FIXME: allow weirder filenames
    sink.set_property(
        "location",
        &path.to_str().expect("non-utf string").to_value(),
    )?;

    let video_info =
        gst_video::VideoInfo::new(gst_video::VideoFormat::Bgra, WIDTH as u32, HEIGHT as u32)
            .fps(gst::Fraction::new(FPS as i32, 1))
            .build()
            .expect("failed to create video info");

    let v_src = v_src
        .dynamic_cast::<gst_app::AppSrc>()
        .expect("failed to get video src");
    v_src.set_caps(Some(&video_info.to_caps().unwrap()));
    v_src.set_property_format(gst::Format::Time); // FIXME: what does this mean?

    let a_src = a_src
        .dynamic_cast::<gst_app::AppSrc>()
        .expect("failed to get audio src");
    let audio_info =
        gst_audio::AudioInfo::new(gst_audio::AudioFormat::S16le, SAMPLE_RATE as u32, 1)
            .build()
            .expect("failed to create audio info");
    a_src.set_caps(Some(&audio_info.to_caps().unwrap()));
    a_src.set_property_format(gst::Format::Time); // FIXME: needed?

    // This will be called every time the video source requests data.
    let mut frame_counter = 0;
    let need_data = move |src: &gst_app::AppSrc, _: u32| {
        if frame_counter == frame_count {
            let _ = src.end_of_stream();
            return;
        }

        let time = Time::from_video_frame(frame_counter, FPS);

        // Create a cairo surface and render to it.
        let mut surface = ImageSurface::create(Format::ARgb32, WIDTH as i32, HEIGHT as i32)
            .expect("failed to create surface");
        {
            let mut cr = Context::new(&surface);
            let mut ctx = CairoRenderContext::new(&mut cr);
            ctx.clear(druid::Color::WHITE);
            // This is copy-paste from DrawingPane. TODO: factor rendering out somewhere
            ctx.with_save(|ctx| {
                ctx.transform(Affine::scale(WIDTH as f64 / 1600.0)); // FIXME
                for (_, curve) in anim.snippets() {
                    ctx.stroke(
                        curve.path_at(time),
                        &curve.curve.color,
                        curve.curve.thickness,
                    );
                }
                Ok(())
            })
            .unwrap();
            ctx.finish().unwrap();
            surface.flush();
        }

        // Create a gst buffer and copy the cairo surface over to it. (TODO: it would be nice to render
        // directly into this buffer, but cairo doesn't seem to safely support rendering into borrowed
        // buffers.)
        let mut gst_buffer =
            gst::Buffer::with_size(video_info.size()).expect("failed to get buffer");
        {
            let gst_buffer_ref = gst_buffer.get_mut().unwrap();
            // Presentation time stamp (i.e. when should this frame be displayed).
            gst_buffer_ref.set_pts(time.as_gst_clock_time());

            let mut data = gst_buffer_ref
                .map_writable()
                .expect("failed to get buffer data");
            data.as_mut_slice()
                .copy_from_slice(&surface.get_data().expect("failed to get surface data"));
        }

        // Ignore the error, since appsrc is supposed to handle it.
        let _ = src.push_buffer(gst_buffer);
        frame_counter += 1;
    };

    let mut cursor = Cursor::new(&audio, time::ZERO, crate::audio::SAMPLE_RATE, 1.0);
    let mut time_us = 0i64;
    let need_audio_data = move |src: &gst_app::AppSrc, size_hint: u32| {
        if cursor.is_finished() {
            let _ = src.end_of_stream();
            return;
        }

        // I'm not sure if this is necessary, but there isn't much documentation on `size_hint` in
        // gstreamer, so just to be sure let's make sure it isn't too small.
        let size = size_hint.max(1024);

        // gstreamer buffers seem to only ever hand out [u8], but we prefer to work with
        // [i16]s. Here, we're doing an extra copy to handle endian-ness and avoid unsafe.
        let mut buf = vec![0i16; size as usize / 2];
        cursor.mix_to_buffer(&audio, &mut buf[..], 1.0);

        let mut gst_buffer = gst::Buffer::with_size(size as usize).expect("audio buffer");
        {
            let gst_buffer_ref = gst_buffer.get_mut().unwrap();
            gst_buffer_ref.set_pts(time_us as u64 * gst::USECOND);
            time_us += (size as i64 / 2 * 1000000) / SAMPLE_RATE as i64;
            let mut data = gst_buffer_ref.map_writable().expect("audio buffer data");
            for (idx, bytes) in data.as_mut_slice().chunks_mut(2).enumerate() {
                bytes.copy_from_slice(&buf[idx].to_le_bytes());
            }
        }
        let _ = src.push_buffer(gst_buffer);
    };

    v_src.set_callbacks(gst_app::AppSrcCallbacks::new().need_data(need_data).build());
    a_src.set_callbacks(
        gst_app::AppSrcCallbacks::new()
            .need_data(need_audio_data)
            .build(),
    );

    Ok(pipeline)
}

// Runs the pipeline (blocking) until it exits or errors.
fn main_loop(pipeline: gst::Pipeline) -> Result<(), anyhow::Error> {
    pipeline.set_state(gst::State::Playing)?;
    let bus = pipeline.get_bus().expect("no bus");

    for msg in bus.iter_timed(gst::CLOCK_TIME_NONE) {
        use gst::MessageView::*;

        match msg.view() {
            Eos(..) => break,
            Error(err) => {
                pipeline.set_state(gst::State::Null)?;

                return Err(PipelineError::from(err).into());
            }
            _ => {}
        }
    }

    pipeline.set_state(gst::State::Null)?;
    dbg!("finished encoding loop");
    Ok(())
}

pub fn do_encode_blocking(cmd: crate::cmd::ExportCmd) -> Result<(), anyhow::Error> {
    let end_time = cmd
        .snippets
        .last_draw_time()
        .max(cmd.audio_snippets.end_time())
        + time::Diff::from_micros(200000);
    let num_frames = end_time.as_video_frame(FPS);
    main_loop(create_pipeline(
        cmd.snippets,
        cmd.audio_snippets,
        num_frames as u32,
        &cmd.filename,
    )?)
}

pub fn encode_blocking(cmd: crate::cmd::ExportCmd) {
    if let Err(e) = do_encode_blocking(cmd) {
        log::error!("error {}", e);
    }
}
