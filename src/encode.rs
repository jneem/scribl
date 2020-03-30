use cairo::{Context, Format, ImageSurface};
use druid::RenderContext;
use gst::prelude::*;
use gstreamer as gst;
use gstreamer_app as gst_app;
use gstreamer_video as gst_video;
use piet_cairo::CairoRenderContext;
use std::path::Path;

use crate::data::SnippetsData;

const FPS: u32 = 30;
const WIDTH: i32 = 1600;
const HEIGHT: i32 = 1200;

fn create_pipeline(
    anim: SnippetsData,
    frame_count: u32,
    path: &Path,
) -> Result<gst::Pipeline, anyhow::Error> {
    let pipeline = gst::Pipeline::new(None);
    let src = gst::ElementFactory::make("appsrc", Some("source"))?;
    let convert = gst::ElementFactory::make("videoconvert", Some("convert"))?;
    let encode = gst::ElementFactory::make("av1enc", Some("encode"))?;
    let mux = gst::ElementFactory::make("mp4mux", Some("mux"))?;
    let sink = gst::ElementFactory::make("filesink", Some("sink"))?;

    pipeline.add_many(&[&src, &convert, &encode, &mux, &sink])?;
    gst::Element::link_many(&[&src, &convert, &encode, &mux, &sink])?;

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

    let src = src
        .dynamic_cast::<gst_app::AppSrc>()
        .expect("failed to get src");
    src.set_caps(Some(&video_info.to_caps().unwrap()));
    src.set_property_format(gst::Format::Time); // FIXME: what does this mean?

    // This will be called every time the source requests data.
    let mut frame_counter: u32 = 0;
    let need_data = move |src: &gst_app::AppSrc, _: u32| {
        println!("rendering frame {}", frame_counter);
        if frame_counter == frame_count {
            let _ = src.end_of_stream();
            return;
        }

        let time_us = (frame_counter as i64 * 1000000) / (FPS as i64);

        // Create a cairo surface and render to it.
        let mut surface = ImageSurface::create(Format::ARgb32, WIDTH as i32, HEIGHT as i32)
            .expect("failed to create surface");
        {
            let mut cr = Context::new(&surface);
            let mut ctx = CairoRenderContext::new(&mut cr);
            ctx.clear(druid::Color::WHITE);
            // This is copy-paste from DrawingPane. TODO: factor rendering out somewhere
            for (_, curve) in anim.snippets() {
                ctx.stroke(
                    curve.path_at(time_us),
                    &curve.curve.color,
                    curve.curve.thickness,
                );
            }
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
            gst_buffer_ref.set_pts(time_us as u64 * gst::USECOND);

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

    src.set_callbacks(gst_app::AppSrcCallbacks::new().need_data(need_data).build());

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
                return Err(err.get_error().into());
            }
            _ => {}
        }
    }

    pipeline.set_state(gst::State::Null)?;
    Ok(())
}

pub fn encode_blocking(anim: SnippetsData, filename: &Path) -> Result<(), anyhow::Error> {
    main_loop(create_pipeline(anim, 50, filename)?)
}
