use druid::im::Vector;
use druid::kurbo::{BezPath, Circle};
use druid::piet::{LineCap, LineJoin, StrokeStyle};
use druid::widget::{Button, Flex, Label, Slider};
use druid::{
    AppLauncher, Color, Data, Event, Lens, Point, RenderContext, Widget, WidgetExt, WindowDesc,
};

use std::cell::RefCell;
use std::sync::Arc;

#[derive(Clone)]
struct Stroke {
    points: Vec<Point>,
    filtered_points: Vec<Point>,
    thickness: f64,
    path: BezPath,
}

#[derive(Clone, Data, Lens)]
struct State {
    cur_points: Arc<RefCell<Vec<Point>>>,
    strokes: Vector<Arc<Stroke>>,
    hide_points: bool,
    simplify_eps: f64,
    tangent_factor: f64,
}

impl Stroke {
    fn new(points: Vec<Point>, eps: f64, tangent_factor: f64) -> Stroke {
        let indices = scribl_curves::simplify(&points, eps);
        let filtered_points: Vec<Point> = indices.iter().map(|&i| points[i]).collect();
        let curve = scribl_curves::smooth(&filtered_points, tangent_factor);

        Stroke {
            thickness: 2.0,
            points,
            filtered_points,
            path: curve,
        }
    }
}

impl Default for State {
    fn default() -> State {
        State {
            cur_points: Default::default(),
            strokes: Default::default(),
            hide_points: false,
            simplify_eps: 1.0,
            tangent_factor: 0.4,
        }
    }
}

impl State {
    fn clear(&mut self) {
        self.cur_points.borrow_mut().clear();
        self.strokes.clear();
    }

    fn finish_stroke(&mut self) {
        let points: Vec<Point> = std::mem::take(self.cur_points.borrow_mut().as_mut());
        if points.len() < 2 {
            return;
        }
        let stroke = Stroke::new(points, self.simplify_eps, self.tangent_factor);

        self.strokes.push_back(Arc::new(stroke));
    }

    fn recalc_strokes(&mut self) {
        let new_strokes = self
            .strokes
            .iter()
            .map(|s| {
                Arc::new(Stroke::new(
                    s.points.clone(),
                    self.simplify_eps,
                    self.tangent_factor,
                ))
            })
            .collect();
        self.strokes = new_strokes;
    }
}

struct Drawing {}

fn slider(name: &str, min: f64, max: f64) -> impl Widget<f64> {
    Flex::row()
        .with_child(Label::new(name))
        .with_child(Slider::new().with_range(min, max))
        .with_child(Label::dynamic(|x: &f64, _| format!("{:5.2}", x)))
}

pub fn main() {
    let controls = Flex::row()
        .with_child(Button::new("Clear").on_click(|_, data: &mut State, _| {
            data.clear();
        }))
        .with_default_spacer()
        .with_child(
            Button::new("Toggle points").on_click(|_, data: &mut State, _| {
                data.hide_points = !data.hide_points;
            }),
        )
        .with_default_spacer()
        .with_child(Button::new("Refresh").on_click(|_, data: &mut State, _| {
            data.recalc_strokes();
        }))
        .with_default_spacer()
        .with_child(slider("eps", 0.1, 3.0).lens(State::simplify_eps))
        .with_default_spacer()
        .with_child(slider("t_factor", 0.1, 0.9).lens(State::tangent_factor));
    let root = Flex::column()
        .with_flex_child(Drawing {}, 1.0)
        .with_child(controls);
    let window = WindowDesc::new(root)
        .title("Draw!")
        .window_size((400.0, 400.0));

    AppLauncher::with_window(window)
        .launch(State::default())
        .expect("Failed to launch");
}

impl Widget<State> for Drawing {
    fn event(
        &mut self,
        ctx: &mut druid::EventCtx,
        event: &druid::Event,
        data: &mut State,
        _env: &druid::Env,
    ) {
        match event {
            Event::MouseDown(ev) => {
                ctx.set_active(true);
                data.cur_points.borrow_mut().clear();
                data.cur_points.borrow_mut().push(ev.pos);
                ctx.request_paint();
            }
            Event::MouseMove(ev) if ctx.is_active() => {
                data.cur_points.borrow_mut().push(ev.pos);
                ctx.request_paint();
            }
            Event::MouseUp(_) => {
                ctx.set_active(false);
                data.finish_stroke();
                ctx.request_paint();
            }
            _ => {}
        }
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut druid::LifeCycleCtx,
        _event: &druid::LifeCycle,
        _data: &State,
        _env: &druid::Env,
    ) {
    }

    fn update(
        &mut self,
        ctx: &mut druid::UpdateCtx,
        old_data: &State,
        data: &State,
        _env: &druid::Env,
    ) {
        if !data.strokes.ptr_eq(&old_data.strokes) {
            ctx.request_paint();
        }
        if data.hide_points != old_data.hide_points {
            ctx.request_paint();
        }
    }

    fn layout(
        &mut self,
        _ctx: &mut druid::LayoutCtx,
        bc: &druid::BoxConstraints,
        _data: &State,
        _env: &druid::Env,
    ) -> druid::Size {
        bc.max()
    }

    fn paint(&mut self, ctx: &mut druid::PaintCtx, data: &State, _env: &druid::Env) {
        for p in data.cur_points.borrow().iter() {
            ctx.fill(Circle::new(*p, 1.0), &Color::BLACK);
        }

        let style = StrokeStyle::new()
            .line_cap(LineCap::Round)
            .line_join(LineJoin::Round);
        for s in &data.strokes {
            ctx.stroke_styled(&s.path, &Color::BLACK, s.thickness, &style);
            if !data.hide_points {
                for p in &s.points {
                    ctx.fill(Circle::new(*p, s.thickness / 2.0), &Color::BLUE);
                }
                for p in &s.filtered_points {
                    ctx.fill(Circle::new(*p, s.thickness / 2.0), &Color::RED);
                }
            }
        }
    }
}
