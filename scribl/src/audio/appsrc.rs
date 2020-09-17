use anyhow::{anyhow, Result};
use crossbeam_channel::Receiver;
use gst::prelude::*;
use gst_audio::{AudioFormat, AudioInfo};
use gstreamer as gst;
use gstreamer_app as gst_app;
use gstreamer_audio as gst_audio;

use scribl_curves::{Cursor, Time};

use super::{OutputData, SAMPLE_RATE};

/// Creates a gstreamer AppSrc element that mixes our audio and provides it to a gstreamer
/// pipeline.
pub fn create_appsrc(rx: Receiver<OutputData>, name: &str) -> Result<gst::Element> {
    let src = gst::ElementFactory::make("appsrc", Some(name))?;
    let src = src
        .dynamic_cast::<gst_app::AppSrc>()
        .map_err(|_| anyhow!("bug: couldn't cast src to an AppSrc"))?;
    let audio_info = AudioInfo::builder(AudioFormat::S16le, SAMPLE_RATE as u32, 1).build()?;
    src.set_caps(Some(&audio_info.to_caps()?));
    src.set_property_format(gst::Format::Time);
    src.set_stream_type(gst_app::AppStreamType::RandomAccess);

    let mut data = OutputData::new();
    let mut cursor = Cursor::empty(0);
    let mut need_audio_data_inner = move |src: &gst_app::AppSrc,
                                          size_hint: u32|
          -> anyhow::Result<()> {
        for new_data in rx.try_iter() {
            data = new_data;
            let idx = data.start_time.as_audio_idx(SAMPLE_RATE);
            cursor = Cursor::new(data.snips.snippet_spans(), idx, idx);
        }
        if data.forwards() && cursor.is_finished() || !data.forwards() && cursor.current().1 == 0 {
            let _ = src.end_of_stream();
            return Ok(());
        }

        let size = size_hint as usize / 2;

        // gstreamer buffers seem to only ever hand out [u8], but we prefer to work with
        // [i16]s. Here, we're doing an extra copy to handle endian-ness and avoid unsafe.
        let mut buf = vec![0i16; size];
        if data.forwards() {
            let prev_end = cursor.current().1;
            cursor.advance_to(prev_end, prev_end + buf.len());
        } else {
            let prev_start = cursor.current().0;
            cursor.advance_to(prev_start.saturating_sub(buf.len()), prev_start);
        }
        data.snips.mix_to(&cursor, &mut buf[..]);
        let time = Time::from_audio_idx(cursor.current().0, SAMPLE_RATE);

        let mut gst_buffer = gst::Buffer::with_size(size * 2)?;
        {
            let gst_buffer_ref = gst_buffer
                .get_mut()
                .ok_or(anyhow!("couldn't get mut buffer"))?;

            let time = if data.forwards() {
                time
            } else {
                data.start_time + (data.start_time - time)
            };
            gst_buffer_ref.set_pts(gst::ClockTime::from_useconds(time.as_micros() as u64));
            let mut gst_buf = gst_buffer_ref.map_writable()?;
            if data.forwards() {
                for (idx, bytes) in gst_buf.as_mut_slice().chunks_mut(2).enumerate() {
                    bytes.copy_from_slice(&buf[idx].to_le_bytes());
                }
            } else {
                for (idx, bytes) in gst_buf.as_mut_slice().chunks_mut(2).rev().enumerate() {
                    bytes.copy_from_slice(&buf[idx].to_le_bytes());
                }
            }
        }
        let _ = src.push_buffer(gst_buffer);
        Ok(())
    };

    let need_audio_data = move |src: &gst_app::AppSrc, size_hint: u32| {
        if let Err(e) = need_audio_data_inner(src, size_hint) {
            log::error!("error synthesizing audio: {}", e);
        }
    };

    // The seek callback doesn't actually do anything. That's because we reset the cursor position
    // in `start_playing` anyway, and that's the only meaningful seek that ever happens.
    let seek = move |_src: &gst_app::AppSrc, _arg: u64| -> bool { true };

    src.set_callbacks(
        gst_app::AppSrcCallbacks::builder()
            .need_data(need_audio_data)
            .seek_data(seek)
            .build(),
    );
    Ok(src.upcast::<gst::Element>())
}
