use std::any::Any;

use std::time::Instant;

use piet_common::kurbo::{Affine, Point, Rect, Vec2};
use piet_common::{Color, Piet, RenderContext};

use druid_shell::{Application, KeyEvent, WinHandler, WindowBuilder, WindowHandle};

use scribble_curves::{Curve, Effects, LineStyle, SnippetData, Time};

fn make_curve(center: Point) -> Curve {
    let mut ret = Curve::new();
    ret.move_to(
        center,
        Time::from_micros(0),
        LineStyle {
            color: FG_COLOR.clone(),
            thickness: 0.2,
        },
        Effects::default(),
    );
    for i in 0..100_000 {
        let time = Time::from_micros(i * 100);
        let t = i as f64 * 2.0 * std::f64::consts::PI / 1000.0;
        let x = t.cos() * t / 5.0;
        let y = t.sin() * t / 5.0;

        ret.line_to(center + Vec2::new(x, y), time);
    }
    ret
}

fn make_snippet(center: Point) -> SnippetData {
    SnippetData::new(make_curve(center))
}

const BG_COLOR: Color = Color::rgb8(0x27, 0x28, 0x22);
const FG_COLOR: Color = Color::rgb8(0xf0, 0xf0, 0xea);

struct PerfTest {
    handle: WindowHandle,
    snippets: Vec<SnippetData>,
    size: (f64, f64),
    last_time: Instant,
    start_time: Instant,
}

impl WinHandler for PerfTest {
    fn connect(&mut self, handle: &WindowHandle) {
        self.handle = handle.clone();
    }

    fn paint(&mut self, piet: &mut Piet) -> bool {
        let (width, height) = self.size;
        let now = Instant::now();

        let rect = Rect::new(0.0, 0.0, width, height);
        piet.fill(rect, &BG_COLOR);

        let anim_time = Time::from_micros((now - self.start_time).as_micros() as i64);
        piet.with_save(|piet| {
            let trans = Vec2::new(width / 2.0, height / 2.0);
            let scale = width / 300.0;
            dbg!(scale);

            piet.transform(Affine::translate(trans) * Affine::scale(scale));
            piet.clip(Rect::new(-10.0, -10.0, 20.0, 20.0));
            for snip in &self.snippets {
                snip.render(&mut *piet, anim_time);
            }
            Ok(())
        })
        .unwrap();
        println!("{}ms", (now - self.last_time).as_millis());
        self.last_time = now;

        true
    }

    fn command(&mut self, id: u32) {
        match id {
            0x100 => self.handle.close(),
            _ => println!("unexpected id {}", id),
        }
    }

    fn key_down(&mut self, event: KeyEvent) -> bool {
        println!("keydown: {:?}", event);
        false
    }

    fn size(&mut self, width: u32, height: u32) {
        let dpi = self.handle.get_dpi();
        let dpi_scale = dpi as f64 / 96.0;
        let width_f = (width as f64) / dpi_scale;
        let height_f = (height as f64) / dpi_scale;
        self.size = (width_f, height_f);
    }

    fn destroy(&mut self) {
        Application::quit()
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

fn main() {
    let mut app = Application::new(None);
    let mut builder = WindowBuilder::new();
    let perf_test = PerfTest {
        size: Default::default(),
        snippets: vec![
            make_snippet(Point::ZERO),
            make_snippet(Point::new(100.0, 20.0)),
            make_snippet(Point::new(-100.0, -100.0)),
        ],
        handle: Default::default(),
        last_time: Instant::now(),
        start_time: Instant::now(),
    };
    builder.set_handler(Box::new(perf_test));
    builder.set_title("Performance tester");

    let window = builder.build().unwrap();
    window.show();
    app.run();
}
