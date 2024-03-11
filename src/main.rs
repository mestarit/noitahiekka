// HALOOO https://github.com/dust-engine/dust https://dust.rs

fn main() {
    pollster::block_on(run());
}

use noitahiekka::{State, Chunk};
use winit::{
    event::*, event_loop::{ControlFlow, EventLoop}, keyboard::{Key, NamedKey}, window::WindowBuilder
};

pub async fn run() {
    env_logger::init();
    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Poll);
    let window = WindowBuilder::new().build(&event_loop).unwrap();

    let mut state = State::new(&window).await;

    let window = &window;
    let mut i = 0;
    event_loop.run(move |event, elwt| match event {
        Event::WindowEvent {
            ref event,
            window_id,
        } if window_id == window.id() => if !state.input(event) { match event {
            WindowEvent::CloseRequested
            | WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Pressed,
                        logical_key: Key::Named(NamedKey::Escape),
                        ..
                    },
                ..
            } => elwt.exit(),
            WindowEvent::Resized(physical_size) => {
                state.resize(*physical_size);
            }
            WindowEvent::RedrawRequested => {
                state.update();
                if i % 60*4 == 0 {
                    let mut chunk_in = Chunk::default();
                    chunk_in.voxels[1][0][0] = 1;
                    state.start_compute(&chunk_in);
                }
                i += 1;
                if let Some(chunk_out) = state.recv_compute() {
                    eprintln!("Chunk!! {chunk_out:?}");
                }
                match state.render() {
                    Ok(_) => {}
                    // Reconfigure the surface if lost
                    Err(wgpu::SurfaceError::Lost) => state.resize(state.win_size),
                    // The system is out of memory, we should probably quit
                    Err(wgpu::SurfaceError::OutOfMemory) => elwt.exit(),
                    // All other errors (Outdated, Timeout) should be resolved by the next frame
                    Err(e) => eprintln!("{:?}", e),
                }
            }
            _ => {}
        }},
        Event::AboutToWait => {
            // Application update code.

            // Queue a RedrawRequested event.
            //
            // You only need to call this if you've determined that you need to redraw in
            // applications which do not always need to. Applications that redraw continuously
            // can render here instead.
            window.request_redraw();
        },
        _ => {}
    }).unwrap();
}

