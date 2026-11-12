use std::{borrow::Cow, mem::size_of};

use anyhow::{Context as _, Result};
use bytemuck::{Pod, Zeroable};
use sdl3::video::Window;
use wgpu::{
    rwh::{HasDisplayHandle, HasWindowHandle},
    util::DeviceExt,
};

use crate::{
    ecs::DrawItem,
    scripting::{GameSnapshot, GameStatus},
};

const WORLD_HALF_WIDTH: f32 = 5.0;
const WORLD_HEIGHT: f32 = 7.5;

const SHADER: &str = include_str!("../assets/brick_breaker.wgsl");

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 2],
    color: [f32; 4],
}

impl Vertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4];

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

// SDL windows are main-thread-owned, while wgpu's window target requires Send + Sync.
// This wrapper only forwards the raw handles; it does not move or access the SDL window.
struct SdlWindowHandle<'window>(&'window Window);

// SAFETY: The wrapper only borrows SDL's stable native handles for surface creation.
// All SDL window/event operations remain on the main thread.
unsafe impl Send for SdlWindowHandle<'_> {}
// SAFETY: See the Send implementation above. No SDL API is called through shared access.
unsafe impl Sync for SdlWindowHandle<'_> {}

impl HasWindowHandle for SdlWindowHandle<'_> {
    fn window_handle(
        &self,
    ) -> std::result::Result<wgpu::rwh::WindowHandle<'_>, wgpu::rwh::HandleError> {
        self.0.window_handle()
    }
}

impl HasDisplayHandle for SdlWindowHandle<'_> {
    fn display_handle(
        &self,
    ) -> std::result::Result<wgpu::rwh::DisplayHandle<'_>, wgpu::rwh::HandleError> {
        self.0.display_handle()
    }
}

pub struct Renderer<'window> {
    _instance: wgpu::Instance,
    surface: wgpu::Surface<'window>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    adapter_name: String,
}

impl<'window> Renderer<'window> {
    pub async fn new(window: &'window Window) -> Result<Self> {
        let (width, height) = window.size_in_pixels();
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let surface = instance
            .create_surface(SdlWindowHandle(window))
            .map_err(|error| anyhow::anyhow!(error.to_string()))
            .context("create wgpu surface from SDL3 raw window handles")?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .context("no compatible wgpu adapter is available")?;
        let adapter_name = adapter.get_info().name;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Brick Breaker device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .context("request wgpu device")?;

        let mut config = surface
            .get_default_config(&adapter, width.max(1), height.max(1))
            .context("wgpu adapter cannot present to the SDL3 surface")?;
        config.desired_maximum_frame_latency = 2;
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Brick Breaker WGSL shader"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SHADER)),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Brick Breaker pipeline layout"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Brick Breaker rectangle pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Vertex::layout()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        Ok(Self {
            _instance: instance,
            surface,
            device,
            queue,
            config,
            pipeline,
            adapter_name,
        })
    }

    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    pub fn render(&mut self, items: &[DrawItem], game: GameSnapshot) -> Result<()> {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                drop(frame);
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                anyhow::bail!("wgpu surface returned a validation failure")
            }
        };

        let mut vertices = Vec::with_capacity((items.len() + 500) * 6);
        for item in items {
            let center_x = item.x / WORLD_HALF_WIDTH;
            let center_y = item.y / (WORLD_HEIGHT * 0.5) - 1.0;
            let width = item.width / WORLD_HALF_WIDTH;
            let height = item.height / (WORLD_HEIGHT * 0.5);
            push_rectangle(&mut vertices, center_x, center_y, width, height, item.color);
        }

        push_text(
            &mut vertices,
            &format!("SCORE {}", game.score),
            -0.96,
            0.94,
            0.014,
            [0.88, 0.93, 1.0, 1.0],
        );
        push_text(
            &mut vertices,
            &format!("LIVES {}", game.lives),
            0.54,
            0.94,
            0.014,
            [0.88, 0.93, 1.0, 1.0],
        );

        let status_text = match game.status {
            GameStatus::Ready => Some("SPACE TO LAUNCH"),
            GameStatus::Playing => None,
            GameStatus::Victory => Some("VICTORY"),
            GameStatus::GameOver => Some("GAME OVER"),
        };
        if let Some(status) = status_text {
            let scale = 0.024;
            let text_width = status.chars().count() as f32 * 6.0 * scale;
            push_text(
                &mut vertices,
                status,
                -text_width * 0.5,
                0.16,
                scale,
                [1.0, 0.92, 0.64, 1.0],
            );
        }
        if matches!(game.status, GameStatus::Victory | GameStatus::GameOver) {
            push_text(
                &mut vertices,
                "SPACE TO RESTART",
                -0.40,
                -0.02,
                0.016,
                [0.82, 0.88, 0.98, 1.0],
            );
        }

        let vertex_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Brick Breaker frame vertices"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Brick Breaker frame encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Brick Breaker main pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.035,
                            g: 0.055,
                            b: 0.095,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_vertex_buffer(0, vertex_buffer.slice(..));
            pass.draw(0..vertices.len() as u32, 0..1);
        }

        self.queue.submit(Some(encoder.finish()));
        self.queue.present(frame);
        Ok(())
    }
}

fn push_rectangle(
    vertices: &mut Vec<Vertex>,
    center_x: f32,
    center_y: f32,
    width: f32,
    height: f32,
    color: [f32; 4],
) {
    let left = center_x - width * 0.5;
    let right = center_x + width * 0.5;
    let bottom = center_y - height * 0.5;
    let top = center_y + height * 0.5;
    let vertex = |x, y| Vertex {
        position: [x, y],
        color,
    };
    vertices.extend_from_slice(&[
        vertex(left, bottom),
        vertex(right, bottom),
        vertex(right, top),
        vertex(left, bottom),
        vertex(right, top),
        vertex(left, top),
    ]);
}

fn push_text(
    vertices: &mut Vec<Vertex>,
    text: &str,
    start_x: f32,
    top_y: f32,
    scale: f32,
    color: [f32; 4],
) {
    for (character_index, character) in text.chars().enumerate() {
        let glyph = glyph_rows(character);
        let character_x = start_x + character_index as f32 * scale * 6.0;
        for (row, bits) in glyph.iter().enumerate() {
            for column in 0..5 {
                if bits & (1 << (4 - column)) != 0 {
                    let x = character_x + column as f32 * scale + scale * 0.5;
                    let y = top_y - row as f32 * scale - scale * 0.5;
                    push_rectangle(vertices, x, y, scale, scale, color);
                }
            }
        }
    }
}

fn glyph_rows(character: char) -> [u8; 7] {
    match character {
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'G' => [14, 17, 16, 23, 17, 17, 15],
        'I' => [31, 4, 4, 4, 4, 4, 31],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [30, 1, 1, 14, 1, 1, 30],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 16, 30, 1, 1, 30],
        '6' => [14, 16, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 1, 14],
        _ => [0; 7],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangle_builder_generates_two_triangles() {
        let mut vertices = Vec::new();
        push_rectangle(&mut vertices, 0.0, 0.0, 0.5, 0.5, [1.0; 4]);
        assert_eq!(vertices.len(), 6);
    }

    #[test]
    fn glyph_builder_emits_pixels_for_known_characters() {
        let mut vertices = Vec::new();
        push_text(&mut vertices, "SCORE 42", -0.5, 0.5, 0.02, [1.0; 4]);
        assert!(!vertices.is_empty());
    }
}
