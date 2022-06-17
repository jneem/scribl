use anyhow::{anyhow, Error};
use crossbeam_channel::{unbounded, Receiver, Sender};
use druid::kurbo::TranslateScale;
use druid::piet::{Device, ImageFormat};
use druid::{Color, Data, Rect, RenderContext};
use gst::prelude::*;
use gst_video::{VideoFormat, VideoInfo};
use gstreamer as gst;
use gstreamer_app as gst_app;
use gstreamer_video as gst_video;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use scribl_curves::{DrawSnippets, Time, TimeDiff};

use crate::audio::TalkSnippets;

// Note that the aspect ratio here needs to match the aspect ratio
// of the drawing, which is currently fixed at 4:3 in widgets/drawing_pane.rs.
const ASPECT_RATIO: f64 = 4.0 / 3.0;

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
                .src()
                .map(|s| String::from(s.path_string()))
                .unwrap_or_else(|| "None".to_owned()),
            error: e.error().to_string(),
            debug: e.debug().unwrap_or_else(|| "No debug info".to_owned()),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("failed to create gstreamer element \"{ty}\"; probably you are missing a gstreamer plugin")]
struct ElementCreationError {
    ty: String,
    msg: String,
}

fn make_elt(ty: &str, name: &str) -> Result<gst::Element, ElementCreationError> {
    gst::ElementFactory::make(ty, Some(name)).map_err(|e| ElementCreationError {
        ty: ty.to_owned(),
        msg: e.message.to_string(),
    })
}

fn create_pipeline(
    anim: DrawSnippets,
    audio: TalkSnippets,
    frame_count: u32,
    path: &Path,
    config: crate::config::Export,
    progress: Sender<EncodingStatus>,
) -> Result<gst::Pipeline, anyhow::Error> {
    let pipeline = gst::Pipeline::new(None);
    let v_src = make_elt("appsrc", "encode-vsource")?;
    let v_convert = make_elt("videoconvert", "encode-vconvert")?;
    let v_encode = make_elt("x264enc", "encode-vencode")?;
    let v_queue1 = make_elt("queue", "encode-vqueue1")?;
    let v_queue2 = make_elt("queue", "encode-vqueue2")?;
    let audio_output_data = crate::audio::OutputData {
        start_time: Time::ZERO,
        snips: audio,
        velocity: 1.0,
    };
    let (output_tx, output_rx) = unbounded();
    // The unwrap is ok because we know that the receiver is still alive.
    output_tx.send(audio_output_data).unwrap();
    let a_src = crate::audio::create_appsrc(output_rx, "encode-asrc")?;
    let a_convert = make_elt("audioconvert", "encode-aconvert")?;
    let a_encode = make_elt("lamemp3enc", "encode-aencode")?;
    let a_queue1 = make_elt("queue", "encode-aqueue1")?;
    let a_queue2 = make_elt("queue", "encode-aqueue2")?;
    let mux = make_elt("mp4mux", "encode-mux")?;
    let sink = make_elt("filesink", "encode-sink")?;

    v_encode.set_property("bitrate", &config.bitrate);

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
    );

    let height = config.height;
    let width = (height as f64 * ASPECT_RATIO).round() as u32;
    let (fps_frac, fps) = if let Some(f) = gst::Fraction::approximate_f64(config.fps) {
        (f, config.fps)
    } else {
        log::warn!("invalid fps value {}, defaulting to 30.0", config.fps);
        (gst::Fraction::new(30, 1), 30.0)
    };
    let video_info = VideoInfo::builder(VideoFormat::Rgba, width, height)
        .fps(fps_frac)
        .build()?;

    let v_src = v_src
        .dynamic_cast::<gst_app::AppSrc>()
        .map_err(|_| anyhow!("bug: couldn't cast v_src to an AppSrc"))?;
    v_src.set_caps(Some(&video_info.to_caps()?));
    v_src.set_format(gst::Format::Time);

    let (tx, rx) = unbounded();
    // gstreamer's callbacks need Sync, not just Send.
    let tx = Arc::new(std::sync::Mutex::new(tx));
    let tx_clone = Arc::clone(&tx);
    v_src.set_callbacks(
        gst_app::AppSrcCallbacks::builder()
            .need_data(move |_, _| {
                let _ = tx.lock().unwrap().send(RenderLoopCmd::NeedsData);
            })
            .enough_data(move |_| {
                let _ = tx_clone.lock().unwrap().send(RenderLoopCmd::EnoughData);
            })
            .build(),
    );
    std::thread::spawn(move || {
        render_loop(
            rx,
            progress,
            v_src,
            anim,
            width,
            height,
            fps,
            frame_count,
            video_info,
        )
    });

    Ok(pipeline)
}

// Runs the pipeline (blocking) until it exits or errors.
fn main_loop(pipeline: gst::Pipeline) -> Result<(), anyhow::Error> {
    pipeline.set_state(gst::State::Playing)?;
    let bus = pipeline
        .bus()
        .ok_or_else(|| anyhow!("couldn't get pipeline bus"))?;

    for msg in bus.iter_timed(gst::ClockTime::NONE) {
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
    Ok(())
}

enum RenderLoopCmd {
    EnoughData,
    NeedsData,
}

fn render_loop(
    cmd: Receiver<RenderLoopCmd>,
    progress: Sender<EncodingStatus>,
    app_src: gst_app::AppSrc,
    snippets: DrawSnippets,
    width: u32,
    height: u32,
    fps: f64,
    frame_count: u32,
    video_info: VideoInfo,
) -> Result<(), Error> {
    let mut device = Device::new().map_err(|e| anyhow!("failed to get device: {}", e))?;
    let mut bitmap = device
        .bitmap_target(width as usize, height as usize, 1.0)
        .map_err(|e| anyhow!("failed to get bitmap: {}", e))?;
    let mut cursor = snippets.create_cursor(Time::ZERO);
    let transform = TranslateScale::scale(width as f64);

    {
        let mut ctx = bitmap.render_context();
        ctx.clear(None, Color::WHITE);
        ctx.finish()
            .map_err(|e| anyhow!("failed to finish context: {}", e))?;
    }

    for frame_counter in 0..frame_count {
        while let Ok(msg) = cmd.try_recv() {
            match msg {
                RenderLoopCmd::EnoughData => while let RenderLoopCmd::EnoughData = cmd.recv()? {},
                RenderLoopCmd::NeedsData => {}
            }
        }

        // We track encoding progress by the fraction of video frames that we've rendered.  This
        // isn't perfect (what with gstreamer's buffering, etc.), but it's probably good enough.
        let _ = progress.send(EncodingStatus::Encoding {
            frame: frame_counter as u64,
            out_of: frame_count as u64,
        });

        let time = Time::from_video_frame(frame_counter, fps);
        let last_time = cursor.current().0;

        // TODO: we have a cursor for visible snippets, but we could also have a cursor for
        // snippets that might potentially cause a change in the visibility. There should be less
        // of these.
        cursor.advance_to(time.min(last_time), time.max(last_time));
        let mut bbox = Rect::ZERO;
        for b in cursor.bboxes(&snippets) {
            if bbox.area() == 0.0 {
                bbox = b;
            } else {
                // TODO: could be more efficient about redrawing.
                bbox = bbox.union(b);
            }
        }

        bbox = (transform * bbox).expand();

        cursor.advance_to(time, time);
        {
            let mut ctx = bitmap.render_context();
            ctx.with_save(|ctx| {
                ctx.clip(bbox);
                ctx.fill(bbox, &Color::WHITE);
                ctx.transform(transform.into());
                for id in cursor.active_ids() {
                    snippets.snippet(id).render(ctx, time);
                }
                Ok(())
            })
            .map_err(|e| anyhow!("failed to render: {}", e))?;
            ctx.finish()
                .map_err(|e| anyhow!("failed to finish context: {}", e))?;
        }

        // Create a gst buffer and copy our data into it (it would be nice to render directly
        // into this buffer, but druid doesn't seem to support rendering into borrowed buffers).
        let mut gst_buffer = gst::Buffer::with_size(video_info.size())?;
        {
            let gst_buffer_ref = gst_buffer
                .get_mut()
                .ok_or(anyhow!("failed to get mutable buffer"))?;
            // Presentation time stamp (i.e. when should this frame be displayed).
            gst_buffer_ref.set_pts(time.as_gst_clock_time());

            let mut data = gst_buffer_ref.map_writable()?;
            bitmap
                .copy_raw_pixels(ImageFormat::RgbaPremul, &mut data)
                .map_err(|e| anyhow!("failed to get raw pixels: {}", e))?;
        }

        // Ignore the error, since appsrc is supposed to handle it.
        let _ = app_src.push_buffer(gst_buffer);
        // Note that piet-cairo (and probably other backends too) currently only supports
        // RgbaPremul.
    }

    let _ = app_src.end_of_stream();
    Ok(())
}

#[derive(Clone, Data, Debug)]
pub enum EncodingStatus {
    /// We are still encoding, and the parameter is the progress (0.0 at the beginning, 1.0 at the
    /// end).
    Encoding { frame: u64, out_of: u64 },

    /// We finished encoding successfully.
    Finished(#[data(same_fn = "PartialEq::eq")] PathBuf),

    /// Encoding aborted with an error.
    Error(String),
}

pub fn do_encode_blocking(
    cmd: crate::cmd::ExportCmd,
    progress: Sender<EncodingStatus>,
) -> Result<(), anyhow::Error> {
    let end_time = cmd
        .scribl
        .draw
        .last_draw_time()
        .max(cmd.scribl.talk.end_time())
        + TimeDiff::from_micros(200000);
    let num_frames = end_time.as_video_frame(cmd.config.fps);
    main_loop(create_pipeline(
        cmd.scribl.draw,
        cmd.scribl.talk,
        num_frames as u32,
        &cmd.filename,
        cmd.config,
        progress,
    )?)
}

pub fn encode_blocking(cmd: crate::cmd::ExportCmd, progress: Sender<EncodingStatus>) {
    let path = cmd.filename.clone();
    if let Err(e) = do_encode_blocking(cmd, progress.clone()) {
        log::error!("error {}", e);
        let _ = progress.send(EncodingStatus::Error(e.to_string()));
    } else {
        let _ = progress.send(EncodingStatus::Finished(path));
    }
}
