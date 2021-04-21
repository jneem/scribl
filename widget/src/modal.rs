use druid::widget::prelude::*;
use druid::{Color, Data, Point, Rect, Selector, SingleUse, Vec2, WidgetPod};

pub struct ModalHost<T, W> {
    mouse_pos: Point,
    inner: W,
    marker: std::marker::PhantomData<T>,
    modal: Option<WidgetPod<T, Box<dyn Widget<T>>>>,
}

impl ModalHost<(), ()> {
    pub const DISMISS_MODAL: Selector<()> = Selector::new("scribl.dismiss-modal");
}

impl<T> ModalHost<T, ()> {
    pub const SHOW_MODAL: Selector<SingleUse<Box<dyn Widget<T>>>> =
        Selector::new("scribl.show-modal");
}

impl<T, W: Widget<T>> ModalHost<T, W> {
    pub fn new(inner: W) -> ModalHost<T, W> {
        ModalHost {
            mouse_pos: Point::ZERO,
            inner,
            marker: std::marker::PhantomData,
            modal: None,
        }
    }
}

impl<T: Data, W: Widget<T>> Widget<T> for ModalHost<T, W> {
    fn event(&mut self, ctx: &mut EventCtx, ev: &Event, data: &mut T, env: &Env) {
        match ev {
            Event::MouseMove(ev) => {
                self.mouse_pos = ev.pos;
            }
            Event::Command(c) => {
                if let Some(modal) = c.get(ModalHost::SHOW_MODAL) {
                    if self.modal.is_none() {
                        self.modal = Some(WidgetPod::new(modal.take().unwrap()));
                        ctx.children_changed();
                    } else {
                        log::warn!("already showing modal");
                    }
                    ctx.set_handled();
                } else if c.is(ModalHost::DISMISS_MODAL) {
                    if self.modal.is_some() {
                        self.modal = None;
                        ctx.children_changed();
                    } else {
                        log::warn!("not showing modal");
                    }
                    ctx.set_handled();
                }
            }
            _ => {}
        }

        if is_user_input(ev) {
            match self.modal.as_mut() {
                Some(modal) => modal.event(ctx, ev, data, env),
                None => self.inner.event(ctx, ev, data, env),
            }
        } else {
            self.inner.event(ctx, ev, data, env)
        }
    }

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, ev: &LifeCycle, data: &T, env: &Env) {
        if let Some(ref mut modal) = self.modal {
            modal.lifecycle(ctx, ev, data, env);
        }
        self.inner.lifecycle(ctx, ev, data, env)
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: &T, data: &T, env: &Env) {
        if let Some(ref mut modal) = self.modal {
            modal.update(ctx, data, env);
        }
        self.inner.update(ctx, old_data, data, env)
    }

    fn layout(&mut self, ctx: &mut LayoutCtx, bc: &BoxConstraints, data: &T, env: &Env) -> Size {
        let size = self.inner.layout(ctx, bc, data, env);
        if let Some(modal) = self.modal.as_mut() {
            let modal_constraints = BoxConstraints::new(Size::ZERO, size);
            let modal_size = modal.layout(ctx, &modal_constraints, data, env);
            let modal_origin = (size.to_vec2() - modal_size.to_vec2()) / 2.0;
            let modal_frame = Rect::from_origin_size(modal_origin.to_point(), modal_size);
            modal.set_layout_rect(ctx, data, env, modal_frame);
        }
        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        self.inner.paint(ctx, data, env);

        if let Some(modal) = self.modal.as_mut() {
            let frame = ctx.size().to_rect();
            ctx.fill(frame, &Color::BLACK.with_alpha(0.45));
            let modal_rect = modal.layout_rect() + Vec2::new(5.0, 5.0);
            let blur_color = Color::grey8(100);
            ctx.blurred_rect(modal_rect, 5.0, &blur_color);
            modal.paint(ctx, data, env);
        }
    }
}

fn is_user_input(event: &Event) -> bool {
    match event {
        Event::MouseUp(_)
        | Event::MouseDown(_)
        | Event::MouseMove(_)
        | Event::KeyUp(_)
        | Event::KeyDown(_)
        | Event::Paste(_)
        | Event::Wheel(_)
        | Event::Zoom(_) => true,
        _ => false,
    }
}
