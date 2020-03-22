use druid::widget::{Align, Flex};
use druid::{
    BoxConstraints, Env, Event, EventCtx, LayoutCtx, LifeCycle, LifeCycleCtx, PaintCtx, Size,
    UpdateCtx, Widget,
};

use crate::data::ScribbleState;
use crate::widgets::{DrawingPane, Timeline, ToggleButton};

fn rec_button_on(_ctx: &mut EventCtx, data: &mut ScribbleState, _env: &Env) {
    data.start_recording();
}

fn rec_button_off(_ctx: &mut EventCtx, data: &mut ScribbleState, _env: &Env) {
    dbg!("Stopped recording", data.time_us);
    data.stop_recording();
}

fn play_button_on(_ctx: &mut EventCtx, data: &mut ScribbleState, _env: &Env) {
    data.start_playing();
}

fn play_button_off(_ctx: &mut EventCtx, data: &mut ScribbleState, _env: &Env) {
    data.stop_playing();
}

pub struct Root {
    inner: Box<dyn Widget<ScribbleState>>,
}

impl Root {
    pub fn new() -> Root {
        let drawing = DrawingPane::default();
        let rec_button: ToggleButton<ScribbleState> = ToggleButton::new(
            "Rec",
            |state: &ScribbleState| state.action.rec_toggle(),
            &rec_button_on,
            &rec_button_off,
        );
        let play_button = ToggleButton::new(
            "Play",
            |state: &ScribbleState| state.action.play_toggle(),
            &play_button_on,
            &play_button_off,
        );

        let button_row = Flex::row()
            .with_child(rec_button, 0.0)
            .with_child(play_button, 0.0);
        let column = Flex::column()
            .with_child(button_row, 0.0)
            .with_spacer(10.0)
            .with_child(drawing, 1.0)
            .with_spacer(10.0)
            .with_child(Timeline::default(), 0.0);

        Root {
            inner: Box::new(Align::centered(column)),
        }
    }
}

impl Widget<ScribbleState> for Root {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut ScribbleState, env: &Env) {
        self.inner.event(ctx, event, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &ScribbleState,
        data: &ScribbleState,
        env: &Env,
    ) {
        self.inner.update(ctx, old_data, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &ScribbleState,
        env: &Env,
    ) {
        self.inner.lifecycle(ctx, event, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &ScribbleState,
        env: &Env,
    ) -> Size {
        self.inner.layout(ctx, bc, data, env)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &ScribbleState, env: &Env) {
        self.inner.paint(ctx, data, env);
    }
}
