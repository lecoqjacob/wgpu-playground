pub mod canvas_data;
pub mod dsl;
pub mod pipelines;
pub mod shaders;

use instant::Instant;

use bytemuck::{Pod, Zeroable};
use canvas_data::CanvasData;
use glam::Vec2;
use glass::{
    device_context::DeviceConfig,
    pipelines::QuadPipeline,
    wgpu,
    window::{GlassWindow, WindowConfig},
    winit, Glass, GlassApp, GlassConfig, GlassContext, GlassError, RenderData,
};
use pipelines::Pipelines;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

pub const SIM_SIZE: u32 = 1024;
pub const WORK_GROUP_SIZE: u32 = 32;
pub const FPS_60: f32 = 16.0 / 1000.0;

fn config() -> GlassConfig {
    GlassConfig {
        device_config: DeviceConfig {
            power_preference: wgpu::PowerPreference::HighPerformance,
            features: wgpu::Features::PUSH_CONSTANTS
                | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
            limits: wgpu::Limits {
                // Using 32 * 32 work group size
                max_compute_invocations_per_workgroup: 1024,
                ..wgpu::Limits::default()
            },
            backends: wgpu::Backends::all(),
        },
        window_configs: vec![WindowConfig {
            width: SIM_SIZE,
            height: SIM_SIZE,
            exit_on_esc: true,
            present_mode: wgpu::PresentMode::AutoNoVsync,
            ..WindowConfig::default()
        }],
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen(start))]
pub fn run() {
    cfg_if::cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            std::panic::set_hook(Box::new(console_error_panic_hook::hook));
            console_log::init_with_level(log::Level::Info).expect("Couldn't initialize logger");
        } else {
            env_logger::builder()
            .filter_level(log::LevelFilter::Info)
            .filter(Some("wgpu"), log::LevelFilter::Error)
            .filter(Some("naga"), log::LevelFilter::Error)
            .build();
        }
    }

    match Glass::new(GameOfLifeApp::default(), config()).run() {
        Ok(_) => {}
        Err(GlassError::AdapterError) => {
            log::error!("Adapter Error");
        }
        Err(GlassError::WindowError(e)) => {
            log::error!("Window Error: {}", e);
        }
        Err(GlassError::SurfaceError(e)) => {
            log::error!("Surface Error: {}", e);
        }
        Err(GlassError::ImageError(e)) => {
            log::error!("Image Error: {}", e);
        }
        Err(GlassError::DeviceError(e)) => {
            log::error!("Device error: {}", e);
        }
    }
}

#[rustfmt::skip]
const OPENGL_TO_WGPU: glam::Mat4 = glam::Mat4::from_cols_array(&[
    1.0, 0.0, 0.0, 0.0,
    0.0, 1.0, 0.0, 0.0,
    0.0, 0.0, 0.5, 0.0,
    0.0, 0.0, 0.5, 1.0,
]);

pub struct GameOfLifeApp {
    dt_sum: f32,
    num_dts: f32,
    count: usize,
    time: Instant,
    updated_time: Instant,

    drawing: bool,
    cursor_pos: Vec2,
    prev_cursor_pos: Option<Vec2>,

    data: Option<CanvasData>,
    quad_pipeline: Option<QuadPipeline>,
    init_pipeline: Option<wgpu::ComputePipeline>,
    draw_pipeline: Option<wgpu::ComputePipeline>,
    game_of_life_pipeline: Option<wgpu::ComputePipeline>,
}

impl Default for GameOfLifeApp {
    fn default() -> Self {
        Self {
            count: 0,
            dt_sum: 0.0,
            num_dts: 0.0,
            time: Instant::now(),
            updated_time: Instant::now(),

            drawing: false,
            prev_cursor_pos: None,
            cursor_pos: Default::default(),

            data: None,
            quad_pipeline: None,
            init_pipeline: None,
            draw_pipeline: None,
            game_of_life_pipeline: None,
        }
    }
}

impl GameOfLifeApp {
    fn cursor_to_canvas(&self, width: f32, height: f32) -> (Vec2, Vec2) {
        let half_screen = Vec2::new(width, height) / 2.0;
        let current_canvas_pos = self.cursor_pos - half_screen + SIM_SIZE as f32 / 2.0;
        let prev_canvas_pos = self.prev_cursor_pos.unwrap_or(current_canvas_pos) - half_screen
            + SIM_SIZE as f32 / 2.0;
        (current_canvas_pos, prev_canvas_pos)
    }
}

// Think of this like reading a "table of contents".
// - Start is run before event loop
// - Input is run on winit input
// - Update is run every frame
// - Render is run for each window after update every frame
impl GlassApp for GameOfLifeApp {
    fn start(
        &mut self,
        _event_loop: &winit::event_loop::EventLoop<()>,
        context: &mut GlassContext,
    ) {
        // #[cfg(target_arch = "wasm32")]
        // {
        //     use winit::platform::web::WindowExtWebSys;

        //     let window = context.primary_render_window().window();
        //     log::info!("canvas: {:?}", window);
        //     // let canvas = window.canvas();

        //     // let window = web_sys::window().unwrap();
        //     // let document = window.document().unwrap();
        //     // let body = document.body().unwrap();

        //     // // Set a background color for the canvas to make it easier to tell where the canvas is for debugging purposes.
        //     // canvas.style().set_css_text("background-color: crimson;");
        //     // body.append_child(&canvas).unwrap();

        //     // let log_header = document.create_element("h2").unwrap();
        //     // log_header.set_text_content(Some("Event Log"));
        //     // body.append_child(&log_header).unwrap();

        //     // let log_list = document.create_element("ul").unwrap();
        //     // body.append_child(&log_list).unwrap();

        //     //     // Winit prevents sizing with CSS, so we have to set
        //     //     // the size manually when on web.
        //     //     use winit::dpi::PhysicalSize;
        //     //     window.set_inner_size(PhysicalSize::new(450, 400));

        //     //     use winit::platform::web::WindowExtWebSys;
        //     //     web_sys::window()
        //     //         .and_then(|win| win.document())
        //     //         .and_then(|doc| {
        //     //             let dst = doc.get_element_by_id("wasm-example")?;
        //     //             let canvas = web_sys::Element::from(window.canvas());
        //     //             dst.append_child(&canvas).ok()?;
        //     //             Some(())
        //     //         })
        //     //         .expect("Couldn't append canvas to document body.");
        // }

        // Create pipelines
        let Pipelines {
            init_pipeline,
            game_of_life_pipeline,
            draw_pipeline,
        } = Pipelines::load(context);

        let quad_pipeline = QuadPipeline::new(context.device(), GlassWindow::surface_format());
        self.data = Some(CanvasData::create(
            context,
            &quad_pipeline,
            &init_pipeline,
            &draw_pipeline,
        ));

        self.quad_pipeline = Some(quad_pipeline);
        self.init_pipeline = Some(init_pipeline);
        self.draw_pipeline = Some(draw_pipeline);
        self.game_of_life_pipeline = Some(game_of_life_pipeline);

        init_game_of_life(self, context);
    }

    fn input(
        &mut self,
        _context: &mut GlassContext,
        _event_loop: &winit::event_loop::EventLoopWindowTarget<()>,
        event: &winit::event::Event<()>,
    ) {
        handle_inputs(self, event);
    }

    fn update(&mut self, context: &mut GlassContext) {
        run_update(self, context);
    }

    fn render(&mut self, _context: &GlassContext, render_data: RenderData) {
        render(self, render_data);
    }
}

fn run_update(app: &mut GameOfLifeApp, context: &mut GlassContext) {
    let now = Instant::now();
    app.dt_sum += (now - app.time).as_secs_f32();
    app.num_dts += 1.0;
    if app.num_dts == 100.0 {
        // Set fps
        context.primary_render_window().window().set_title(&format!(
            "Game Of Life: {:.2}",
            1.0 / (app.dt_sum / app.num_dts)
        ));
        app.num_dts = 0.0;
        app.dt_sum = 0.0;
    }
    app.time = Instant::now();

    // Use only single command queue
    let mut encoder = context
        .device()
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Computes"),
        });

    // Update 60fps
    if (app.time - app.updated_time).as_secs_f32() > FPS_60 {
        update_game_of_life(app, context, &mut encoder);
        app.updated_time = app.time;
    }

    if app.drawing {
        draw_game_of_life(app, context, &mut encoder);
    }

    // Update prev cursor pos
    app.prev_cursor_pos = Some(app.cursor_pos);

    // Submit
    context.queue().submit(Some(encoder.finish()));
}

fn render(app: &mut GameOfLifeApp, render_data: RenderData) {
    let GameOfLifeApp {
        data,
        quad_pipeline,
        ..
    } = app;

    let canvas_data = data.as_ref().unwrap();
    let quad_pipeline = quad_pipeline.as_ref().unwrap();
    let RenderData {
        encoder,
        frame,
        window,
        ..
    } = render_data;

    let (width, height) = {
        let size = window.window().inner_size();
        (size.width as f32, size.height as f32)
    };

    let view = frame
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());

    {
        let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            depth_stencil_attachment: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: true,
                },
            })],
        });

        quad_pipeline.draw(
            &mut rpass,
            &canvas_data.canvas_bind_group,
            [0.0; 4],
            camera_projection([width, height]).to_cols_array_2d(),
            canvas_data.canvas.size,
        );
    }
}

fn handle_inputs(app: &mut GameOfLifeApp, event: &winit::event::Event<()>) {
    if let winit::event::Event::WindowEvent { event, .. } = event {
        match event {
            winit::event::WindowEvent::CursorMoved { position, .. } => {
                app.cursor_pos = Vec2::new(position.x as f32, position.y as f32);
            }
            winit::event::WindowEvent::MouseInput {
                button: winit::event::MouseButton::Left,
                state,
                ..
            } => {
                app.drawing = state == &winit::event::ElementState::Pressed;
            }
            _ => (),
        }
    }
}

fn draw_game_of_life(
    app: &mut GameOfLifeApp,
    context: &mut GlassContext,
    encoder: &mut wgpu::CommandEncoder,
) {
    let (width, height) = {
        let size = context.primary_render_window().window().inner_size();
        (size.width as f32, size.height as f32)
    };

    let (end, start) = app.cursor_to_canvas(width, height);
    let GameOfLifeApp {
        data,
        draw_pipeline,
        ..
    } = app;

    let data = data.as_ref().unwrap();
    let draw_pipeline = draw_pipeline.as_ref().unwrap();
    let pc = GameOfLifePushConstants::new(start, end, 10.0);

    let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: Some("draw_game_of_life"),
    });
    cpass.set_pipeline(draw_pipeline);
    cpass.set_bind_group(0, &data.draw_bind_group, &[]);
    cpass.set_push_constants(0, bytemuck::cast_slice(&[pc]));
    cpass.dispatch_workgroups(SIM_SIZE / WORK_GROUP_SIZE, SIM_SIZE / WORK_GROUP_SIZE, 1);
}

fn update_game_of_life(
    app: &mut GameOfLifeApp,
    context: &GlassContext,
    encoder: &mut wgpu::CommandEncoder,
) {
    let GameOfLifeApp {
        data,
        game_of_life_pipeline,
        ..
    } = app;

    let data = data.as_ref().unwrap();
    let game_of_life_pipeline = game_of_life_pipeline.as_ref().unwrap();

    let (canvas, data_in) = if app.count % 2 == 0 {
        (&data.canvas.views[0], &data.data_in.views[0])
    } else {
        (&data.data_in.views[0], &data.canvas.views[0])
    };

    let update_bind_group = context
        .device()
        .create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Update Bind Group"),
            layout: &game_of_life_pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(canvas),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(data_in),
                },
            ],
        });

    let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: Some("Update"),
    });
    cpass.set_pipeline(game_of_life_pipeline);
    cpass.set_bind_group(0, &update_bind_group, &[]);
    cpass.dispatch_workgroups(SIM_SIZE / WORK_GROUP_SIZE, SIM_SIZE / WORK_GROUP_SIZE, 1);

    app.count += 1;
}

fn init_game_of_life(app: &mut GameOfLifeApp, context: &mut GlassContext) {
    let GameOfLifeApp {
        data,
        init_pipeline,
        ..
    } = app;

    let data = data.as_ref().unwrap();
    let init_pipeline = init_pipeline.as_ref().unwrap();

    let mut encoder = context
        .device()
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

    {
        let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("Init"),
        });
        cpass.set_pipeline(init_pipeline);
        cpass.set_bind_group(0, &data.init_bind_group, &[]);
        cpass.dispatch_workgroups(SIM_SIZE / WORK_GROUP_SIZE, SIM_SIZE / WORK_GROUP_SIZE, 1);
    }
    context.queue().submit(Some(encoder.finish()));
}

// =============================== CAMERA =============================== //

fn camera_projection(screen_size: [f32; 2]) -> glam::Mat4 {
    let half_width = screen_size[0] / 2.0;
    let half_height = screen_size[1] / 2.0;
    OPENGL_TO_WGPU
        * glam::Mat4::orthographic_rh(
            -half_width,
            half_width,
            -half_height,
            half_height,
            0.0,
            1000.0,
        )
}

// =============================== MISC =============================== //

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct GameOfLifePushConstants {
    draw_start: [f32; 2],
    draw_end: [f32; 2],
    draw_radius: f32,
}

impl GameOfLifePushConstants {
    pub fn new(draw_start: Vec2, draw_end: Vec2, draw_radius: f32) -> Self {
        Self {
            draw_radius,
            draw_end: draw_end.to_array(),
            draw_start: draw_start.to_array(),
        }
    }
}