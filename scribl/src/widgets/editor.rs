use crossbeam_channel::Sender;
use druid::widget::{Flex, Scroll};
use druid::{
    theme, BoxConstraints, Command, Data, Env, Event, EventCtx, ExtEventSink, KbKey, KeyEvent,
    LayoutCtx, LifeCycle, LifeCycleCtx, PaintCtx, SingleUse, Size, TimerToken, UpdateCtx, Widget,
    WidgetExt, WidgetId, WindowId,
};
use std::path::PathBuf;
use std::time::Duration;

use scribl_widget::{ModalHost, RadioGroup, Separator, SunkenContainer, ToggleButton, TooltipExt};

use crate::audio::AudioHandle;
use crate::autosave::AutosaveData;
use crate::data::Settings;
use crate::widgets::{
    alert, icons, make_status_bar, AudioIndicator, DrawingPane, Palette, Timeline,
};
use crate::{
    cmd, CurrentAction, DenoiseSetting, EditorState, PenSize, RecordingSpeed, SaveFileData,
};

const AUTOSAVE_INTERVAL: Duration = Duration::from_secs(60);
const ICON_PADDING: f64 = 6.0;
const TOOLBAR_WIDTH: f64 = 52.0;
const SECONDARY_BUTTON_PADDING: f64 = 4.0;

pub struct Editor {
    // Every AUTOSAVE_DURATION, we will attempt to save the current file.
    autosave_timer_id: TimerToken,
    // We won't save the current file if it hasn't changed since the last autosave.
    last_autosave_data: Option<SaveFileData>,
    // We send the autosave data on this channel.
    autosave_tx: Option<Sender<AutosaveData>>,
    // A handle to the audio thread. We initialize this on WidgetAdded, so it should rarely be
    // `None`.
    //
    // The audio state is derived from our EditorState, and our `update` method is where the actual
    // commands get sent to the audio thread.
    audio: Option<AudioHandle>,

    inner: Box<dyn Widget<EditorState>>,
}

fn make_draw_button_group() -> impl Widget<EditorState> {
    let rec_button = ToggleButton::from_icon(
        &icons::VIDEO,
        ICON_PADDING,
        |state: &EditorState, _env: &Env| {
            if state.action.is_recording() {
                "Stop recording (Space)"
            } else {
                "Record a drawing (Space)"
            }
            .to_owned()
        },
        |state: &EditorState| state.action.is_recording(),
        |_, state, _| state.draw(),
        |_, state, _| state.finish_action(),
    );

    let rec_speed_group = RadioGroup::icon_column(
        vec![
            (
                &icons::PAUSE,
                RecordingSpeed::Paused,
                "Draw a static image".into(),
            ),
            (
                &icons::SNAIL,
                RecordingSpeed::Slower,
                "Draw in super-slow motion".into(),
            ),
            (
                &icons::TURTLE,
                RecordingSpeed::Slow,
                "Draw in slow motion".into(),
            ),
            (
                &icons::RABBIT,
                RecordingSpeed::Normal,
                "Draw in real time".into(),
            ),
        ],
        ICON_PADDING,
    )
    .padding(SECONDARY_BUTTON_PADDING)
    .lens(Settings::recording_speed)
    .lens(EditorState::settings);

    let rec_fade_button = ToggleButton::from_icon(
        &icons::FADE_OUT,
        ICON_PADDING,
        |state: &bool, _env: &Env| {
            if *state {
                "Disable fade effect"
            } else {
                "Enable fade effect"
            }
            .to_owned()
        },
        |&b: &bool| b,
        |_, data, _| *data = true,
        |_, data, _| *data = false,
    )
    .padding(SECONDARY_BUTTON_PADDING)
    .lens(Settings::fade_enabled)
    .lens(EditorState::settings);

    let draw_button_group = Flex::column()
        .with_child(rec_button)
        .with_spacer(5.0)
        .with_child(rec_speed_group)
        .with_spacer(5.0)
        .with_child(rec_fade_button)
        .padding(5.0)
        .background(theme::BACKGROUND_LIGHT)
        .rounded(theme::BUTTON_BORDER_RADIUS);

    draw_button_group
}

fn make_pen_group() -> impl Widget<EditorState> {
    // 8.0 is twice (the default value of ) BUTTON_ICON_PADDING, so this serves to make the palette
    //   width the same as the pen_size_group width. TODO: make the padding values more convenient
    //   to customize, so this isn't some magic number
    let palette = Palette::new()
        .lens(Settings::palette)
        .padding(10.0)
        .background(theme::BACKGROUND_LIGHT)
        .rounded(theme::BUTTON_BORDER_RADIUS);

    let pen_size_group = RadioGroup::icon_column(
        vec![
            (&icons::BIG_CIRCLE, PenSize::Big, "BIG PEN! (Q)".into()),
            (
                &icons::MEDIUM_CIRCLE,
                PenSize::Medium,
                "Medium pen (W)".into(),
            ),
            (&icons::SMALL_CIRCLE, PenSize::Small, "Small pen (E)".into()),
        ],
        ICON_PADDING,
    )
    .padding(10.0)
    .background(theme::BACKGROUND_LIGHT)
    .rounded(theme::BUTTON_BORDER_RADIUS);

    Flex::column()
        .with_child(palette)
        .with_default_spacer()
        .with_child(pen_size_group.lens(Settings::pen_size))
        .lens(EditorState::settings)
}

fn make_audio_button_group() -> impl Widget<EditorState> {
    let audio_indicator =
        AudioIndicator::new()
            .padding(ICON_PADDING)
            .tooltip(|state: &EditorState, _env: &Env| {
                if state.action.is_recording_audio() {
                    "Stop recording (Shift+Space)"
                } else {
                    "Start recording audio (Shift+Space)"
                }
                .to_owned()
            });
    let rec_audio_button = ToggleButton::from_widget(
        audio_indicator,
        |state: &EditorState| state.action.is_recording_audio(),
        |_, state, _| state.talk(),
        |_, state, _| state.finish_action(),
    );

    let noise_group = RadioGroup::icon_column(
        vec![
            (
                &icons::NOISE,
                DenoiseSetting::DenoiseOff,
                "Disable denoising".into(),
            ),
            (
                &icons::REMOVE_NOISE,
                DenoiseSetting::DenoiseOn,
                "Enable denoising but not speech detection".into(),
            ),
            (
                &icons::SPEECH,
                DenoiseSetting::Vad,
                "Enable denoising and speech detection".into(),
            ),
        ],
        ICON_PADDING,
    )
    .padding(SECONDARY_BUTTON_PADDING)
    .lens(Settings::denoise_setting)
    .lens(EditorState::settings);

    Flex::column()
        .with_child(rec_audio_button)
        .with_spacer(5.0)
        .with_child(noise_group)
        .padding(5.0)
        .background(theme::BACKGROUND_LIGHT)
        .rounded(theme::BUTTON_BORDER_RADIUS)
}

impl Editor {
    pub fn new() -> Editor {
        let drawing = DrawingPane::default();
        let play_button = ToggleButton::from_icon(
            &icons::PLAY,
            ICON_PADDING,
            |state: &EditorState, _env: &Env| {
                if state.action.is_playing() {
                    "Pause playback (Enter)"
                } else {
                    "Play back the animation (Enter)"
                }
                .to_owned()
            },
            |state: &EditorState| state.action.is_playing(),
            |_, state, _| state.play(),
            |_, state, _| state.finish_action(),
        );

        let draw_button_group = make_draw_button_group();
        let audio_button_group = make_audio_button_group();

        let watch_button_group = Flex::column()
            .with_child(play_button)
            .padding(5.0)
            .background(theme::BACKGROUND_LIGHT)
            .rounded(theme::BUTTON_BORDER_RADIUS);

        let button_col = Flex::column()
            .with_child(draw_button_group)
            .with_default_spacer()
            .with_child(audio_button_group)
            .with_default_spacer()
            .with_child(watch_button_group);
        let button_col = Scroll::new(button_col).vertical().fix_width(TOOLBAR_WIDTH);
        let pen_col = Scroll::new(make_pen_group())
            .vertical()
            .fix_width(TOOLBAR_WIDTH);
        let timeline_id = WidgetId::next();
        let timeline = Timeline::new().with_id(timeline_id);
        /*
        TODO: Issues with split:
         - can't get timeline to use up the vertical space it has available
         - can't set a reasonable default initial size
        let drawing_and_timeline = Split::horizontal(drawing.padding(10.0), timeline)
            .draggable(true).debug_paint_layout();
        */
        let column = Flex::column()
            .with_flex_child(
                SunkenContainer::new(
                    Flex::row()
                        .with_child(button_col)
                        .with_flex_child(drawing, 1.0)
                        .with_child(pen_col),
                ),
                1.0,
            )
            .with_child(Separator::new().height(10.0).color(theme::BACKGROUND_LIGHT))
            .with_child(timeline)
            .with_child(make_status_bar())
            .background(theme::BACKGROUND_DARK);

        Editor {
            inner: Box::new(ModalHost::new(column)),
            autosave_timer_id: TimerToken::INVALID,
            audio: None,
            last_autosave_data: None,
            autosave_tx: None,
        }
    }
}

impl Editor {
    fn handle_key_down(
        &mut self,
        ctx: &mut EventCtx,
        ev: &KeyEvent,
        data: &mut EditorState,
        _env: &Env,
    ) {
        // If they push another non-shift key while holding down the arrow, cancel the scanning.
        if let CurrentAction::Scanning(speed) = data.action {
            let direction = if speed > 0.0 {
                KbKey::ArrowRight
            } else {
                KbKey::ArrowLeft
            };
            if ev.key != direction && ev.key != KbKey::Shift {
                data.finish_action();
            }
            ctx.set_handled();
            if ev.key == KbKey::ArrowRight || ev.key == KbKey::ArrowLeft {
                return;
            }
        }

        match ev.key {
            KbKey::ArrowRight | KbKey::ArrowLeft => {
                let speed = if ev.mods.shift() { 3.0 } else { 1.5 };
                let dir = if ev.key == KbKey::ArrowRight {
                    1.0
                } else {
                    -1.0
                };
                let velocity = speed * dir;
                if data.action.is_idle() || data.action.is_scanning() {
                    data.scan(velocity);
                    ctx.request_anim_frame();
                }
                ctx.set_handled();
            }
            KbKey::Shift if data.action.is_scanning() => {
                data.scan(3.0 * data.action.time_factor().signum());
            }
            _ => {}
        }
    }

    fn handle_key_up(
        &mut self,
        ctx: &mut EventCtx,
        ev: &KeyEvent,
        data: &mut EditorState,
        _env: &Env,
    ) {
        match &ev.key {
            KbKey::ArrowRight | KbKey::ArrowLeft => {
                if data.action.is_scanning() {
                    data.finish_action();
                }
                ctx.set_handled();
            }
            KbKey::Shift => {
                if data.action.is_scanning() {
                    data.scan(1.5 * data.action.time_factor().signum());
                }
            }
            KbKey::ArrowUp => ctx.submit_command(cmd::SELECT_SNIPPET_ABOVE),
            KbKey::ArrowDown => ctx.submit_command(cmd::SELECT_SNIPPET_BELOW),
            KbKey::Character(s) if !ev.mods.shift() && !ev.mods.ctrl() && !ev.mods.alt() => {
                match s.chars().next().unwrap() {
                    c @ '0'..='9' => {
                        // Select the corresponding color.
                        let num = c.to_digit(10).unwrap_or(0) as usize;
                        // '1' is the first color, '0' is the last.
                        let idx = (num + 9) % 10;
                        // If there is no color at that index, just fail silently.
                        let _ = data.settings.palette.try_select_idx(idx);
                    }
                    'q' => data.settings.pen_size = PenSize::Big,
                    'w' => data.settings.pen_size = PenSize::Medium,
                    'e' => data.settings.pen_size = PenSize::Small,
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn export(&self, ctx: &mut EventCtx, data: &mut EditorState, export: cmd::ExportCmd) {
        if data.status.in_progress.encoding.is_some() {
            log::warn!("already encoding, not doing another one");
        } else {
            // This is a little wasteful, but it's probably fine. We spin up a thread to
            // translate between the Receiver that encode_blocking sends to, and the
            // ExtEventSink that sends commands to us.
            let export = export.clone();
            let (tx, rx) = crossbeam_channel::unbounded();
            let window_id = ctx.window_id();
            let ext_cmd = ctx.get_external_handle();
            std::thread::spawn(move || {
                while let Ok(msg) = rx.recv() {
                    let _ = ext_cmd.submit_command(cmd::ENCODING_STATUS, Box::new(msg), window_id);
                }
            });
            std::thread::spawn(move || crate::encode::encode_blocking(export, tx));
        }
    }

    fn handle_command(
        &mut self,
        ctx: &mut EventCtx,
        cmd: &Command,
        data: &mut EditorState,
        _env: &Env,
    ) -> bool {
        let ret = if let Some(snip_cmd) = cmd.get(cmd::ADD_TALK_SNIPPET) {
            let snip = snip_cmd.snip.clone();
            let prev_state = data.undo_state();
            data.selected_snippet = Some(data.scribl.add_talk_snippet(snip).into());
            data.push_undo_state(prev_state.with_time(snip_cmd.orig_start), "add audio");
            true
        } else if let Some(time) = cmd.get(cmd::WARP_TIME_TO) {
            if data.action.is_idle() {
                data.warp_time_to(*time);
            } else {
                log::warn!("not warping: state is {:?}", data.action)
            }
            true
        } else if let Some(info) = cmd.get(cmd::EXPORT) {
            let mut path = info.path().to_owned();
            if path.extension().is_none() {
                path.set_extension("mp4");
            }
            let export = cmd::ExportCmd {
                scribl: data.scribl.clone(),
                filename: path,
                config: data.config.export.clone(),
            };
            self.export(ctx, data, export);
            true
        } else if cmd.is(druid::commands::SAVE_FILE_AS) || cmd.is(druid::commands::SAVE_FILE) {
            let mut path = if let Some(info) = cmd.get(druid::commands::SAVE_FILE_AS) {
                info.path().to_owned()
            } else if let Some(path) = data.save_path.as_ref() {
                path.to_owned()
            } else {
                log::error!("no save path, not saving");
                return false;
            };
            if path.extension().is_none() {
                path.set_extension("scb");
            }

            data.status.in_progress.saving = Some(path.clone());
            spawn_async_save(
                ctx.get_external_handle(),
                SaveFileData::from_editor_state(data),
                path,
                ctx.window_id(),
            );
            true
        } else if cmd.is(druid::commands::OPEN_FILE) {
            if data.status.in_progress.loading.is_some() {
                log::error!("not loading, already loading");
            } else {
                let info = cmd.get_unchecked(druid::commands::OPEN_FILE);
                data.status.in_progress.loading = Some(info.path().to_owned());
                spawn_async_load(
                    ctx.get_external_handle(),
                    info.path().to_owned(),
                    ctx.window_id(),
                );
                data.set_loading();
            }
            true
        } else if cmd.is(cmd::FINISHED_ASYNC_LOAD) {
            let result = cmd.get_unchecked(cmd::FINISHED_ASYNC_LOAD);
            data.update_load_status(result);
            if let Ok(save_data) = &result.save_data {
                *data = EditorState::from_save_file(save_data.clone(), data.config.clone());
                data.save_path = Some(result.path.clone());
            }
            true
        } else if cmd.is(cmd::FINISHED_ASYNC_SAVE) {
            let result = cmd.get_unchecked(cmd::FINISHED_ASYNC_SAVE);
            data.update_save_status(result);
            if !result.autosave && result.error.is_none() {
                data.save_path = Some(result.path.clone());
            }
            true
        } else if cmd.is(cmd::ENCODING_STATUS) {
            let status = cmd.get_unchecked(cmd::ENCODING_STATUS);
            data.update_encoding_status(status);
            true
        } else if let Some(status) = cmd.get(cmd::RECORDING_AUDIO_STATUS) {
            let vad = data.settings.denoise_setting != DenoiseSetting::Vad
                || status.vad >= data.config.audio_input.vad_threshold;
            data.input_loudness = if vad {
                status.loudness as f64
            } else {
                -f64::INFINITY
            };
            true
        } else {
            false
        };
        ret
    }
}

fn spawn_async_save(ext_cmd: ExtEventSink, save_data: SaveFileData, path: PathBuf, id: WindowId) {
    std::thread::spawn(move || {
        let result = save_data.save_to_path(&path);
        let _ = ext_cmd.submit_command(
            cmd::FINISHED_ASYNC_SAVE,
            Box::new(cmd::AsyncSaveResult {
                path,
                data: save_data,
                error: result.err().map(|e| e.to_string()),
                autosave: false,
            }),
            id,
        );
    });
}

fn spawn_async_load(ext_cmd: ExtEventSink, path: PathBuf, id: WindowId) {
    std::thread::spawn(move || {
        let data = cmd::AsyncLoadResult {
            path: path.clone(),
            save_data: SaveFileData::load_from_path(&path).map_err(|e| e.to_string()),
        };
        let _ = ext_cmd.submit_command(cmd::FINISHED_ASYNC_LOAD, Box::new(data), id);
    });
}

impl Widget<EditorState> for Editor {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut EditorState, env: &Env) {
        match event {
            Event::WindowConnected => {
                ctx.request_focus();
                ctx.request_paint();
            }
            Event::Command(cmd) => {
                let handled = self.handle_command(ctx, cmd, data, env);
                if handled {
                    ctx.set_handled();
                }
            }
            Event::KeyDown(ev) => self.handle_key_down(ctx, ev, data, env),
            Event::KeyUp(ev) => self.handle_key_up(ctx, ev, data, env),
            Event::Timer(tok) if tok == &self.autosave_timer_id => {
                let autosave_data = SaveFileData::from_editor_state(data);
                if !self.last_autosave_data.same(&Some(autosave_data.clone())) {
                    let autosave_data = AutosaveData {
                        data: autosave_data.clone(),
                        path: data.save_path.clone(),
                    };
                    if let Some(tx) = &self.autosave_tx {
                        if let Err(e) = tx.send(autosave_data) {
                            log::error!("failed to send autosave data: {}", e);
                        }
                    }
                }
                self.last_autosave_data = Some(autosave_data);
                self.autosave_timer_id = ctx.request_timer(AUTOSAVE_INTERVAL);
            }
            Event::AnimFrame(_) => {
                if data.action.time_factor() != 0.0 {
                    data.update_time();
                }
            }
            Event::WindowCloseRequested => {
                if matches!(data.action, CurrentAction::WaitingToExit) {
                    // By not handling the request, we're telling druid to really close it.
                } else if data.changed_since_last_save() {
                    ctx.submit_command(ModalHost::SHOW_MODAL.with(SingleUse::new(Box::new(
                        alert::make_unsaved_changes_alert(),
                    ))));
                    ctx.set_handled();
                } else {
                    data.action = CurrentAction::WaitingToExit;
                    ctx.submit_command(ModalHost::SHOW_MODAL.with(SingleUse::new(Box::new(
                        alert::make_waiting_to_exit_alert(),
                    ))));
                    ctx.set_handled();
                }
            }
            _ => {}
        }
        self.inner.event(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &EditorState,
        data: &EditorState,
        env: &Env,
    ) {
        if data.action.time_factor() != 0.0 {
            ctx.request_anim_frame();
        }
        self.inner.update(ctx, old_data, data, env);

        let old_audio_state = old_data.audio_state();
        let new_audio_state = data.audio_state();
        if let Some(audio) = &mut self.audio {
            audio.update(old_audio_state, new_audio_state);
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &EditorState,
        env: &Env,
    ) {
        match event {
            LifeCycle::WidgetAdded => {
                self.autosave_tx = Some(crate::autosave::spawn_autosave_thread(
                    ctx.get_external_handle(),
                    ctx.window_id(),
                ));
                self.autosave_timer_id = ctx.request_timer(AUTOSAVE_INTERVAL);
                self.audio = Some(AudioHandle::initialize_audio(
                    ctx.get_external_handle(),
                    ctx.widget_id().into(),
                ));
            }
            _ => {}
        }
        self.inner.lifecycle(ctx, event, data, env);
        if data.action.time_factor() != 0.0 {
            ctx.request_anim_frame();
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &EditorState,
        env: &Env,
    ) -> Size {
        self.inner.layout(ctx, bc, data, env)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &EditorState, env: &Env) {
        self.inner.paint(ctx, data, env);
    }
}
