use druid::widget::{Align, Flex};
use druid::{
    theme, BoxConstraints, Color, Command, Data, Env, Event, EventCtx, ExtEventSink, KbKey,
    KeyEvent, LayoutCtx, LifeCycle, LifeCycleCtx, PaintCtx, SingleUse, Size, TimerToken, UpdateCtx,
    Widget, WidgetExt, WidgetId, WindowId,
};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::Duration;

use scribl_curves::Time;

use crate::autosave::AutosaveData;
use crate::cmd;
use crate::editor_state::{
    CurrentAction, DenoiseSetting, EditorState, MaybeSnippetId, PenSize, RecordingSpeed,
};
use crate::save_state::SaveFileData;
use crate::widgets::tooltip::{ModalHost, TooltipExt};
use crate::widgets::{
    alert, icons, make_status_bar, make_timeline, DrawingPane, LabelledContainer, Palette,
    ToggleButton, ToggleButtonState,
};

const AUTOSAVE_INTERVAL: Duration = Duration::from_secs(60);

pub struct Editor {
    // Every AUTOSAVE_DURATION, we will attempt to save the current file.
    autosave_timer_id: TimerToken,
    // We won't save the current file if it hasn't changed since the last autosave.
    last_autosave_data: Option<SaveFileData>,
    // We send the autosave data on this channel.
    autosave_tx: Option<Sender<AutosaveData>>,

    // A command sender that we can hand out to other threads, in order that they can send us
    // commands. It's a bit tricky to initialize this (we need to create the `AppLauncher` before
    // we can get one, but that needs a `WindowDesc`, which needs the widget tree). Therefore, we
    // make it an option and we initialize it by using the `ExtEventSink` to send a command
    // containing a copy of the `ExtEventSink` :)
    ext_cmd: Option<ExtEventSink>,

    inner: Box<dyn Widget<EditorState>>,
}

fn make_draw_button_group() -> impl Widget<EditorState> {
    let rec_button = ToggleButton::new(
        &icons::VIDEO,
        24.0,
        |state: &EditorState| state.action.rec_toggle(),
        |ctx, _, _| ctx.submit_command(cmd::DRAW, None),
        |ctx, _, _| ctx.submit_command(cmd::STOP, None),
    )
    .tooltip(|state: &EditorState, _env: &Env| {
        if state.action.rec_toggle() == ToggleButtonState::ToggledOn {
            "Stop recording (Space)"
        } else {
            "Record a drawing (Space)"
        }
        .to_owned()
    });

    let rec_speed_group = crate::widgets::radio_icon::make_radio_icon_group(
        24.0,
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
    );

    let rec_fade_button = ToggleButton::new(
        &icons::FADE_OUT,
        24.0,
        |&b: &bool| b.into(),
        |_, data, _| *data = true,
        |_, data, _| *data = false,
    )
    .tooltip(|state: &bool, _env: &Env| {
        if *state {
            "Disable fade effect"
        } else {
            "Enable fade effect"
        }
        .to_owned()
    })
    .lens(EditorState::fade_enabled);

    let palette = Palette::new(24.0)
        .border(theme::BORDER_LIGHT, crate::BUTTON_GROUP_BORDER_WIDTH)
        // TODO: Get from the theme
        .rounded(5.0)
        .lens(EditorState::palette);

    let pen_size_group = crate::widgets::radio_icon::make_radio_icon_group(
        24.0,
        vec![
            (&icons::BIG_CIRCLE, PenSize::Big, "BIG PEN! (Q)".into()),
            (
                &icons::MEDIUM_CIRCLE,
                PenSize::Medium,
                "Medium pen (W)".into(),
            ),
            (&icons::SMALL_CIRCLE, PenSize::Small, "Small pen (E)".into()),
        ],
    );

    let draw_button_group = Flex::row()
        .with_child(rec_button)
        .with_spacer(10.0)
        .with_child(rec_speed_group.lens(EditorState::recording_speed))
        .with_spacer(10.0)
        .with_child(pen_size_group.lens(EditorState::pen_size))
        .with_spacer(10.0)
        .with_child(palette)
        .with_spacer(10.0)
        .with_child(rec_fade_button)
        .padding(5.0);
    let draw_button_group = LabelledContainer::new(draw_button_group, "Draw")
        .border_color(Color::WHITE)
        .corner_radius(druid::theme::BUTTON_BORDER_RADIUS)
        .padding(5.0);

    draw_button_group
}

fn make_audio_button_group() -> impl Widget<EditorState> {
    let rec_audio_button = ToggleButton::new(
        &icons::MICROPHONE,
        24.0,
        |state: &EditorState| state.action.rec_audio_toggle(),
        |ctx, _, _| ctx.submit_command(cmd::TALK, None),
        |ctx, _, _| ctx.submit_command(cmd::STOP, None),
    )
    .tooltip(|state: &EditorState, _env: &Env| {
        if state.action.rec_audio_toggle() == ToggleButtonState::ToggledOn {
            "Stop recording (Shift+Space)"
        } else {
            "Start recording audio (Shift+Space)"
        }
        .to_owned()
    });

    let noise_group = crate::widgets::radio_icon::make_radio_icon_group(
        24.0,
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
    );

    let audio_button_group = Flex::row()
        .with_child(rec_audio_button)
        .with_spacer(10.0)
        .with_child(noise_group.lens(EditorState::denoise_setting))
        .padding(5.0);

    LabelledContainer::new(audio_button_group, "Talk")
        .border_color(Color::WHITE)
        .corner_radius(druid::theme::BUTTON_BORDER_RADIUS)
        .padding(5.0)
}

impl Editor {
    pub fn new() -> Editor {
        let drawing = DrawingPane::default();
        let play_button = ToggleButton::new(
            &icons::PLAY,
            24.0,
            |state: &EditorState| state.action.play_toggle(),
            |ctx, _, _| ctx.submit_command(cmd::PLAY, None),
            |ctx, _, _| ctx.submit_command(cmd::STOP, None),
        )
        .tooltip(|state: &EditorState, _env: &Env| {
            if state.action.play_toggle() == ToggleButtonState::ToggledOn {
                "Pause playback (Enter)"
            } else {
                "Play back the animation (Enter)"
            }
            .to_owned()
        });

        let draw_button_group = make_draw_button_group();
        let audio_button_group = make_audio_button_group();

        let watch_button_group = Flex::row().with_child(play_button).padding(5.0);
        let watch_button_group = LabelledContainer::new(watch_button_group, "Watch")
            .border_color(Color::WHITE)
            .corner_radius(druid::theme::BUTTON_BORDER_RADIUS)
            .padding(5.0);

        let button_row = Flex::row()
            .with_child(draw_button_group)
            .with_child(audio_button_group)
            .with_child(watch_button_group)
            .with_flex_spacer(1.0);
        let timeline_id = WidgetId::next();
        let timeline = make_timeline().with_id(timeline_id);
        /*
        TODO: Issues with split:
         - can't get timeline to use up the vertical space it has available
         - can't set a reasonable default initial size
        let drawing_and_timeline = Split::horizontal(drawing.padding(10.0), timeline)
            .draggable(true).debug_paint_layout();
        */
        let column = Flex::column()
            .with_child(button_row)
            .with_flex_child(drawing.padding(10.0), 1.0)
            .with_child(timeline)
            .with_child(make_status_bar());

        Editor {
            inner: Box::new(ModalHost::new(Align::centered(column))),
            autosave_timer_id: TimerToken::INVALID,
            last_autosave_data: None,
            autosave_tx: None,
            ext_cmd: None,
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
        // If they push another key while holding down the arrow, cancel the scanning.
        if let CurrentAction::Scanning(speed) = data.action {
            let direction = if speed > 0.0 {
                KbKey::ArrowRight
            } else {
                KbKey::ArrowLeft
            };
            if ev.key != direction {
                data.stop_scanning();
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
                    data.stop_scanning();
                }
                ctx.set_handled();
            }
            KbKey::ArrowUp => ctx.submit_command(cmd::SELECT_SNIPPET_ABOVE, None),
            KbKey::ArrowDown => ctx.submit_command(cmd::SELECT_SNIPPET_BELOW, None),
            KbKey::Character(s) if !ev.mods.shift() && !ev.mods.ctrl() && !ev.mods.alt() => {
                match s.chars().next().unwrap() {
                    c @ '0'..='9' => {
                        // Select the corresponding color.
                        let num = c.to_digit(10).unwrap_or(0) as usize;
                        // '1' is the first color, '0' is the last.
                        let idx = (num + 9) % 10;
                        // If there is no color at that index, just fail silently.
                        let _ = data.palette.try_select_idx(idx);
                    }
                    'q' => data.pen_size = PenSize::Big,
                    'w' => data.pen_size = PenSize::Medium,
                    'e' => data.pen_size = PenSize::Small,
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn handle_command(
        &mut self,
        ctx: &mut EventCtx,
        cmd: &Command,
        data: &mut EditorState,
        _env: &Env,
    ) -> bool {
        // TODO: change to match if/when that is supported.
        let ret = if cmd.is(cmd::ADD_SNIPPET) {
            let prev_state = data.undo_state();
            let snip = cmd.get_unchecked(cmd::ADD_SNIPPET);
            let (new_snippets, new_id) = data.snippets.with_new_snippet(snip.clone());
            data.snippets = new_snippets;
            data.selected_snippet = new_id.into();
            data.push_undo_state(prev_state.with_time(snip.start_time()), "add drawing");
            ctx.set_menu(crate::menus::make_menu(data));
            true
        } else if cmd.is(cmd::DELETE_SNIPPET) {
            let id = cmd.get_unchecked(cmd::DELETE_SNIPPET);
            if let Some(id) = id.as_draw().or(data.selected_snippet.as_draw()) {
                let prev_state = data.undo_state();
                let new_snippets = data.snippets.without_snippet(id);
                data.snippets = new_snippets;
                if data.selected_snippet == id.into() {
                    data.selected_snippet = MaybeSnippetId::None;
                }
                data.push_undo_state(prev_state, "delete drawing");
                ctx.set_menu(crate::menus::make_menu(data));
            } else if let Some(id) = id.as_audio().or(data.selected_snippet.as_audio()) {
                let prev_state = data.undo_state();
                let new_snippets = data.audio_snippets.without_snippet(id);
                data.audio_snippets = new_snippets;
                if data.selected_snippet == id.into() {
                    data.selected_snippet = MaybeSnippetId::None;
                }
                data.push_undo_state(prev_state, "delete audio");
                ctx.set_menu(crate::menus::make_menu(data));
            } else {
                log::error!("No snippet id to delete");
            }
            true
        } else if cmd.is(cmd::ADD_AUDIO_SNIPPET) {
            let prev_state = data.undo_state();
            let snip = cmd.get_unchecked(cmd::ADD_AUDIO_SNIPPET);
            data.audio_snippets = data.audio_snippets.with_new_snippet(snip.clone());
            data.push_undo_state(prev_state.with_time(snip.start_time()), "add audio");
            ctx.set_menu(crate::menus::make_menu(data));
            true
        } else if cmd.is(cmd::APPEND_NEW_SEGMENT) {
            let prev_state = data.undo_state();
            let seg = cmd.get_unchecked(cmd::APPEND_NEW_SEGMENT);
            let start_time = seg.start_time().unwrap_or(Time::ZERO);
            data.add_segment_to_snippet(seg.clone());
            data.push_transient_undo_state(prev_state.with_time(start_time), "add stroke");
            ctx.set_menu(crate::menus::make_menu(data));
            true
        } else if cmd.is(cmd::CHOOSE_COLOR) {
            let color = cmd.get_unchecked(cmd::CHOOSE_COLOR);
            data.palette.select(color);
            true
        } else if cmd.is(cmd::EXPORT) {
            let export = cmd.get_unchecked(cmd::EXPORT);

            if data.status.in_progress.encoding.is_some() {
                log::warn!("already encoding, not doing another one");
            } else if let Some(ext_cmd) = self.ext_cmd.as_ref().cloned() {
                // This is a little wasteful, but it's probably fine. We spin up a thread to
                // translate between the Receiver that encode_blocking sends to, and the
                // ExtEventSink that sends commands to us.
                let export = export.clone();
                let (tx, rx) = std::sync::mpsc::channel();
                let window_id = ctx.window_id();
                std::thread::spawn(move || {
                    while let Ok(msg) = rx.recv() {
                        let _ =
                            ext_cmd.submit_command(cmd::ENCODING_STATUS, Box::new(msg), window_id);
                    }
                });
                std::thread::spawn(move || crate::encode::encode_blocking(export, tx));
            }

            true
        } else if cmd.is(cmd::SET_MARK) {
            let prev_state = data.undo_state();
            let time = cmd.get_unchecked(cmd::SET_MARK).unwrap_or(data.time());
            data.mark = Some(time);
            data.push_undo_state(prev_state, "set mark");
            ctx.set_menu(crate::menus::make_menu(data));
            true
        } else if cmd.is(cmd::TRUNCATE_SNIPPET) {
            if let Some(id) = data.selected_snippet.as_draw() {
                let prev_state = data.undo_state();
                data.snippets = data.snippets.with_truncated_snippet(id, data.time());
                data.push_undo_state(prev_state, "truncate drawing");
                ctx.set_menu(crate::menus::make_menu(data));
            } else {
                log::error!("cannot truncate, nothing selected");
            }
            true
        } else if cmd.is(cmd::LERP_SNIPPET) {
            if let (Some(mark_time), Some(id)) = (data.mark, data.selected_snippet.as_draw()) {
                let prev_state = data.undo_state();
                data.snippets = data.snippets.with_new_lerp(id, data.time(), mark_time);
                data.warp_time_to(mark_time);
                data.push_undo_state(prev_state, "warp drawing");
                ctx.set_menu(crate::menus::make_menu(data));
            } else {
                log::error!(
                    "cannot lerp, mark time {:?}, selected {:?}",
                    data.mark,
                    data.selected_snippet
                );
            }
            true
        } else if cmd.is(druid::commands::UNDO) {
            data.undo();
            ctx.set_menu(crate::menus::make_menu(data));
            ctx.request_paint();
            true
        } else if cmd.is(druid::commands::REDO) {
            data.redo();
            ctx.set_menu(crate::menus::make_menu(data));
            ctx.request_paint();
            true
        } else if cmd.is(cmd::PLAY) {
            if data.action.is_idle() {
                data.start_playing();
                ctx.request_anim_frame();
            } else {
                log::error!("can't play, current action is {:?}", data.action);
            }
            ctx.set_menu(crate::menus::make_menu(data));
            true
        } else if cmd.is(cmd::DRAW) {
            if data.action.is_idle() {
                let prev_state = data.undo_state();
                // We don't request_anim_frame here because recording starts paused. Instead, we do
                // it in `DrawingPane` when the time actually starts.
                data.start_recording(data.recording_speed.factor());
                data.push_transient_undo_state(prev_state, "start drawing");
            } else {
                log::error!("can't draw, current action is {:?}", data.action);
            }
            ctx.set_menu(crate::menus::make_menu(data));
            true
        } else if cmd.is(cmd::TALK) {
            if data.action.is_idle() {
                data.start_recording_audio();
                ctx.request_anim_frame();
            } else {
                log::error!("can't talk, current action is {:?}", data.action);
            }
            ctx.set_menu(crate::menus::make_menu(data));
            true
        } else if cmd.is(cmd::STOP) {
            match data.action {
                CurrentAction::Playing => data.stop_playing(),
                CurrentAction::WaitingToRecord(_) | CurrentAction::Recording(_) => {
                    if let Some(new_snippet) = data.stop_recording() {
                        ctx.submit_command(Command::new(cmd::ADD_SNIPPET, new_snippet), None);
                    }
                }
                CurrentAction::RecordingAudio(_) => {
                    let snip = data.stop_recording_audio();
                    ctx.submit_command(Command::new(cmd::ADD_AUDIO_SNIPPET, snip), None);
                }
                _ => {}
            }
            ctx.set_menu(crate::menus::make_menu(data));
            true
        } else if cmd.is(cmd::UPDATE_TIME) {
            data.update_time();
            true
        } else if cmd.is(cmd::WARP_TIME_TO) {
            if data.action.is_idle() {
                data.warp_time_to(*cmd.get_unchecked(cmd::WARP_TIME_TO));
            } else {
                log::warn!("not warping: state is {:?}", data.action)
            }
            true
        } else if cmd.is(druid::commands::SAVE_FILE) {
            let path = if let Some(info) = cmd.get_unchecked(druid::commands::SAVE_FILE) {
                info.path().to_owned()
            } else if let Some(path) = data.save_path.as_ref() {
                path.to_owned()
            } else {
                log::error!("no save path, not saving");
                return false;
            };

            // Note that we use the SAVE_FILE command for both saving and
            // exporting, and we decide which to do based on the file
            // extension.
            match path.extension().and_then(|e| e.to_str()) {
                Some("mp4") => {
                    let export = cmd::ExportCmd {
                        snippets: data.snippets.clone(),
                        audio_snippets: data.audio_snippets.clone(),
                        filename: path.to_owned(),
                        config: data.config.export.clone(),
                    };
                    ctx.submit_command(Command::new(cmd::EXPORT, export), None);
                }
                Some("scb") => {
                    data.status.in_progress.saving = Some(path.clone());
                    self.spawn_async_save(
                        SaveFileData::from_editor_state(data),
                        path,
                        ctx.window_id(),
                    );
                }
                _ => {
                    log::error!("unknown extension! Trying to save anyway");
                    data.status.in_progress.saving = Some(path.clone());
                    self.spawn_async_save(
                        SaveFileData::from_editor_state(data),
                        path,
                        ctx.window_id(),
                    );
                }
            }
            true
        } else if cmd.is(druid::commands::OPEN_FILE) {
            if data.status.in_progress.loading.is_some() {
                log::error!("not loading, already loading");
            } else {
                let info = cmd.get_unchecked(druid::commands::OPEN_FILE);
                data.status.in_progress.loading = Some(info.path().to_owned());
                self.spawn_async_load(info.path().to_owned(), ctx.window_id());
                data.set_loading();
            }
            true
        } else if cmd.is(druid::commands::CLOSE_WINDOW) {
            log::info!("close window command");
            true
        } else if cmd.is(cmd::INITIALIZE_EVENT_SINK) {
            let ext_cmd = cmd.get_unchecked(cmd::INITIALIZE_EVENT_SINK);
            self.ext_cmd = Some(ext_cmd.clone());
            self.autosave_tx = Some(crate::autosave::spawn_autosave_thread(
                ext_cmd.clone(),
                ctx.window_id(),
            ));
            true
        } else if cmd.is(cmd::FINISHED_ASYNC_LOAD) {
            let result = cmd.get_unchecked(cmd::FINISHED_ASYNC_LOAD);
            data.update_load_status(result);
            if let Ok(save_data) = &result.save_data {
                *data = EditorState::from_save_file(save_data.clone());
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
        } else if cmd.is(cmd::ZOOM_IN) {
            data.zoom = (data.zoom * 1.25).min(crate::editor_state::MAX_ZOOM);
            true
        } else if cmd.is(cmd::ZOOM_OUT) {
            data.zoom = (data.zoom / 1.25).max(1.0);
            true
        } else if cmd.is(cmd::ZOOM_RESET) {
            data.zoom = 1.0;
            true
        } else if cmd.is(cmd::REQUEST_CLOSE_WINDOW) {
            // TODO: note that we can't intercept it (yet) when the system tries to close our
            // window; this is currently only when they close via the menu.
            if data.changed_since_last_save() {
                ctx.submit_command(
                    ModalHost::SHOW_MODAL.with(SingleUse::new(Box::new(
                        alert::make_unsaved_changes_alert(),
                    ))),
                    None,
                );
            } else {
                ctx.submit_command(
                    ModalHost::SHOW_MODAL.with(SingleUse::new(Box::new(
                        alert::make_waiting_to_exit_alert(),
                    ))),
                    None,
                );
            }
            true
        } else {
            false
        };
        ret
    }

    fn spawn_async_save(&mut self, save_data: SaveFileData, path: PathBuf, id: WindowId) {
        if let Some(ext_cmd) = self.ext_cmd.as_ref().cloned() {
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
    }

    fn spawn_async_load(&mut self, path: PathBuf, id: WindowId) {
        if let Some(ext_cmd) = self.ext_cmd.as_ref().cloned() {
            std::thread::spawn(move || {
                let data = cmd::AsyncLoadResult {
                    path: path.clone(),
                    save_data: SaveFileData::load_from_path(&path).map_err(|e| e.to_string()),
                };
                let _ = ext_cmd.submit_command(cmd::FINISHED_ASYNC_LOAD, Box::new(data), id);
            });
        }
    }
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
        self.inner.update(ctx, old_data, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &EditorState,
        env: &Env,
    ) {
        match event {
            LifeCycle::WidgetAdded => self.autosave_timer_id = ctx.request_timer(AUTOSAVE_INTERVAL),
            LifeCycle::AnimFrame(_) => {
                // We're not allowed to update the data in lifecycle, so on each animation frame we
                // send ourselves a command to update the current time.
                if data.action.time_factor() != 0.0 {
                    ctx.submit_command(cmd::UPDATE_TIME, ctx.widget_id());
                }
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
