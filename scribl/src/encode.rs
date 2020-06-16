use anyhow::anyhow;
use druid::Data;
use gst::prelude::*;
use gstreamer as gst;
use gstreamer_app as gst_app;
use gstreamer_video as gst_video;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use scribl_curves::{Cursor, SnippetsData, Time, TimeDiff};

use crate::audio::AudioSnippetsData;
use crate::editor_state::StatusMsg;
use crate::imagebuf::ImageBuf;

const FPS: f64 = 30.0;
// Note that the aspect ratio here needs to match the aspect ratio
// of the drawing, which is currently fixed at 4:3 in widgets/drawing_pane.rs.
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
    progress: Sender<StatusMsg>,
) -> Result<gst::Pipeline, anyhow::Error> {
    let pipeline = gst::Pipeline::new(None);
    let v_src = gst::ElementFactory::make("appsrc", Some("encode-vsource"))?;
    let v_convert = gst::ElementFactory::make("videoconvert", Some("encode-vconvert"))?;
    let v_encode = gst::ElementFactory::make("vp9enc", Some("encode-vencode"))?;
    let v_queue1 = gst::ElementFactory::make("queue", Some("encode-vqueue1"))?;
    let v_queue2 = gst::ElementFactory::make("queue", Some("encode-vqueue2"))?;
    let audio_output_data = crate::audio::OutputData {
        cursor: Cursor::new(audio.snippet_spans(), 0, 0),
        snips: audio,
        forwards: true,
    };
    let a_src =
        crate::audio::create_appsrc(Arc::new(Mutex::new(audio_output_data)), "encode-asrc")?;
    let a_convert = gst::ElementFactory::make("audioconvert", Some("encode-aconvert"))?;
    let a_encode = gst::ElementFactory::make("vorbisenc", Some("encode-aencode"))?;
    let a_queue1 = gst::ElementFactory::make("queue", Some("encode-aqueue1"))?;
    let a_queue2 = gst::ElementFactory::make("queue", Some("encode-aqueue2"))?;
    let mux = gst::ElementFactory::make("webmmux", Some("encode-mux"))?;
    let sink = gst::ElementFactory::make("filesink", Some("encode-sink"))?;

    pipeline.add_many(&[&v_src, &v_convert, &v_encode, &v_queue1, &v_queue2])?;
    pipeline.add_many(&[&a_src, &a_convert, &a_encode, &a_queue1, &a_queue2])?;
    pipeline.add_many(&[&mux, &sink])?;
    gst::Element::link_many(&[&v_src, &v_queue1, &v_convert, &v_encode, &v_queue2, &mux])?;
    gst::Element::link_many(&[&a_src, &a_queue1, &a_convert, &a_encode, &a_queue2, &mux])?;
    gst::Element::link(&mux, &sink)?;

    // TODO: allow weirder filenames
    sink.set_property(
        "location",
        &path
            .to_str()
            .ok_or(anyhow!("this filename is too weird"))?
            .to_value(),
    )?;

    let video_info =
        gst_video::VideoInfo::new(gst_video::VideoFormat::Rgba, WIDTH as u32, HEIGHT as u32)
            .fps(gst::Fraction::new(FPS as i32, 1))
            .build()?;

    let v_src = v_src
        .dynamic_cast::<gst_app::AppSrc>()
        .map_err(|_| anyhow!("bug: couldn't cast v_src to an AppSrc"))?;
    v_src.set_caps(Some(&video_info.to_caps()?));
    v_src.set_property_format(gst::Format::Time); // FIXME: what does this mean?

    // This will be called every time the video source requests data.
    let mut frame_counter = 0;
    let mut image = ImageBuf::new(WIDTH as usize, HEIGHT as usize, &anim);
    let mut need_data_inner = move |src: &gst_app::AppSrc| -> anyhow::Result<()> {
        // We track encoding progress by the fraction of video frames that we've rendered.  This
        // isn't perfect (what with gstreamer's buffering, etc.), but it's probably good enough.
        let _ = progress
            .send(EncodingStatus::Encoding(frame_counter as f64 / frame_count as f64).into());
        if frame_counter == frame_count {
            let _ = src.end_of_stream();
            return Ok(());
        }

        let time = Time::from_video_frame(frame_counter, FPS);
        image.render(&anim, time)?;

        // Create a gst buffer and copy our data into it (TODO: it would be nice to render directly
        // into this buffer, but druid doesn't seem to support rendering into borrowed buffers).
        let mut gst_buffer = gst::Buffer::with_size(video_info.size())?;
        {
            let gst_buffer_ref = gst_buffer
                .get_mut()
                .ok_or(anyhow!("failed to get mutable buffer"))?;
            // Presentation time stamp (i.e. when should this frame be displayed).
            gst_buffer_ref.set_pts(time.as_gst_clock_time());

            let mut data = gst_buffer_ref.map_writable()?;
            data.as_mut_slice().copy_from_slice(image.pixel_data());
        }

        // Ignore the error, since appsrc is supposed to handle it.
        let _ = src.push_buffer(gst_buffer);
        frame_counter += 1;
        Ok(())
    };

    let need_data = move |src: &gst_app::AppSrc, _: u32| {
        if let Err(e) = need_data_inner(src) {
            log::error!("error rendering frame: {}", e);
        }
    };

    v_src.set_callbacks(gst_app::AppSrcCallbacks::new().need_data(need_data).build());
    Ok(pipeline)
}

// Runs the pipeline (blocking) until it exits or errors.
fn main_loop(pipeline: gst::Pipeline) -> Result<(), anyhow::Error> {
    pipeline.set_state(gst::State::Playing)?;
    let bus = pipeline
        .get_bus()
        .ok_or_else(|| anyhow!("couldn't get pipeline bus"))?;

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

#[derive(Clone, Data, Debug)]
pub enum EncodingStatus {
    /// We are still encoding, and the parameter is the progress (0.0 at the beginning, 1.0 at the
    /// end).
    Encoding(f64),

    /// We finished encoding successfully.
    Finished(#[data(same_fn = "PartialEq::eq")] PathBuf),

    /// Encoding aborted with an error.
    Error(String),
}

pub fn do_encode_blocking(
    cmd: crate::cmd::ExportCmd,
    progress: Sender<StatusMsg>,
) -> Result<(), anyhow::Error> {
    let end_time = cmd
        .snippets
        .last_draw_time()
        .max(cmd.audio_snippets.end_time())
        + TimeDiff::from_micros(200000);
    let num_frames = end_time.as_video_frame(FPS);
    main_loop(create_pipeline(
        cmd.snippets,
        cmd.audio_snippets,
        num_frames as u32,
        &cmd.filename,
        progress,
    )?)
}

pub fn encode_blocking(cmd: crate::cmd::ExportCmd, progress: Sender<StatusMsg>) {
    let path = cmd.filename.clone();
    if let Err(e) = do_encode_blocking(cmd, progress.clone()) {
        log::error!("error {}", e);
        let _ = progress.send(EncodingStatus::Error(e.to_string()).into());
    } else {
        let _ = progress.send(EncodingStatus::Finished(path).into());
    }
}
