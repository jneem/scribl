use cairo::{Context, Format, ImageSurface};
use druid_shell::piet::{Color, RenderContext};
use piet_cairo::CairoRenderContext;

use scribble_curves as curves;

use curves::{time, SnippetsData};

const WIDTH: i32 = 1600;
const HEIGHT: i32 = 1200;

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let path = &args[1];
    let file = std::fs::File::open(path).unwrap();
    let snippets: SnippetsData = serde_json::from_reader(file).unwrap();

    let mut time = time::ZERO;
    let diff = time::Diff::from_micros(16_000);
    let surface = ImageSurface::create(Format::ARgb32, WIDTH as i32, HEIGHT as i32)
        .expect("failed to create surface");

    for i in 0..1000 {
        if i % 10 == 0 {
            println!("rendering frame {}", i);
        }
        let mut cr = Context::new(&surface);
        let mut ctx = CairoRenderContext::new(&mut cr);
        ctx.clear(Color::WHITE);
        for (_, curve) in snippets.snippets() {
            curve.render(&mut ctx, time);
        }
        ctx.finish().unwrap();
        surface.flush();

        time += diff;
    }
}
