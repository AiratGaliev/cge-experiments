use std::{borrow::Cow, mem::size_of, sync::Arc};

use anyhow::{Context as _, Result};
use bytemuck::{Pod, Zeroable};
use winit::window::Window;

use crate::ecs::{DrawCircle, DrawRectangle, DrawText};

const WORLD_HALF_WIDTH: f32 = 5.0;
const WORLD_HEIGHT: f32 = 7.5;
const WORLD_ASPECT_RATIO: f32 = (WORLD_HALF_WIDTH * 2.0) / WORLD_HEIGHT;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Viewport {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

fn calculate_viewport(surface_width: u32, surface_height: u32) -> Viewport {
    let surface_width = surface_width.max(1) as f32;
    let surface_height = surface_height.max(1) as f32;

    if surface_width / surface_height > WORLD_ASPECT_RATIO {
        let width = surface_height * WORLD_ASPECT_RATIO;
        Viewport {
            x: (surface_width - width) * 0.5,
            y: 0.0,
            width,
            height: surface_height,
        }
    } else {
        let height = surface_width / WORLD_ASPECT_RATIO;
        Viewport {
            x: 0.0,
            y: (surface_height - height) * 0.5,
            width: surface_width,
            height,
        }
    }
}

const SHADER: &str = include_str!("../assets/flat_color_2d.wgsl");

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

pub struct Renderer {
    _instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    adapter_name: String,
    vertices: Vec<Vertex>,
    vertex_buffer: Option<wgpu::Buffer>,
    vertex_buffer_capacity: u64,
}

impl Renderer {
    pub async fn new(window: Arc<Window>) -> Result<Self> {
        let size = window.inner_size();
        let (width, height) = (size.width, size.height);
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let surface = instance
            .create_surface(window.clone())
            .map_err(|error| anyhow::anyhow!(error.to_string()))
            .context("create wgpu surface from the winit window raw handles")?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })
            .await
            .context("no compatible wgpu adapter is available")?;
        let adapter_name = adapter.get_info().name;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Codogma native runtime device"),
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
            .context("wgpu adapter cannot present to the winit window surface")?;
        config.desired_maximum_frame_latency = 2;
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Codogma 2D WGSL shader"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SHADER)),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Codogma 2D pipeline layout"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Codogma 2D rectangle pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(Vertex::layout())],
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
            vertices: Vec::new(),
            vertex_buffer: None,
            vertex_buffer_capacity: 0,
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

    pub fn render(
        &mut self,
        rectangles: &[DrawRectangle],
        circles: &[DrawCircle],
        texts: &[DrawText],
    ) -> Result<()> {
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

        let required_vertices = (rectangles.len() + circles.len() * 24 + texts.len() * 24) * 6;
        self.vertices.clear();
        self.vertices.reserve(required_vertices);
        for item in rectangles {
            let center_x = item.x / WORLD_HALF_WIDTH;
            let center_y = item.y / (WORLD_HEIGHT * 0.5);
            let width = item.width / WORLD_HALF_WIDTH;
            let height = item.height / (WORLD_HEIGHT * 0.5);
            push_rectangle(
                &mut self.vertices,
                center_x,
                center_y,
                width,
                height,
                item.color,
            );
        }
        for item in circles {
            push_circle(
                &mut self.vertices,
                item.x / WORLD_HALF_WIDTH,
                item.y / (WORLD_HEIGHT * 0.5),
                item.radius / WORLD_HALF_WIDTH,
                item.radius / (WORLD_HEIGHT * 0.5),
                item.color,
            );
        }
        for item in texts {
            push_text(
                &mut self.vertices,
                &item.value,
                item.x,
                item.y,
                item.scale,
                item.color,
            );
        }

        let vertex_bytes = bytemuck::cast_slice(&self.vertices);
        let required_buffer_size =
            (vertex_bytes.len() as u64).max(std::mem::size_of::<Vertex>() as u64);
        if self.vertex_buffer_capacity < required_buffer_size {
            self.vertex_buffer_capacity = required_buffer_size.next_power_of_two();
            self.vertex_buffer = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("CGE native runtime reusable frame vertices"),
                size: self.vertex_buffer_capacity,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let vertex_buffer = self
            .vertex_buffer
            .as_ref()
            .expect("vertex buffer is allocated before rendering");
        if !vertex_bytes.is_empty() {
            self.queue.write_buffer(vertex_buffer, 0, vertex_bytes);
        }

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("CGE native runtime frame encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("CGE native runtime render pass"),
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
            let viewport = calculate_viewport(self.config.width, self.config.height);
            pass.set_viewport(
                viewport.x,
                viewport.y,
                viewport.width,
                viewport.height,
                0.0,
                1.0,
            );
            pass.set_vertex_buffer(0, vertex_buffer.slice(..));
            pass.draw(0..self.vertices.len() as u32, 0..1);
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

fn push_circle(
    vertices: &mut Vec<Vertex>,
    center_x: f32,
    center_y: f32,
    radius_x: f32,
    radius_y: f32,
    color: [f32; 4],
) {
    const SEGMENTS: usize = 24;
    for segment in 0..SEGMENTS {
        let angle_a = std::f32::consts::TAU * segment as f32 / SEGMENTS as f32;
        let angle_b = std::f32::consts::TAU * (segment + 1) as f32 / SEGMENTS as f32;
        vertices.extend_from_slice(&[
            Vertex {
                position: [center_x, center_y],
                color,
            },
            Vertex {
                position: [
                    center_x + angle_a.cos() * radius_x,
                    center_y + angle_a.sin() * radius_y,
                ],
                color,
            },
            Vertex {
                position: [
                    center_x + angle_b.cos() * radius_x,
                    center_y + angle_b.sin() * radius_y,
                ],
                color,
            },
        ]);
    }
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
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 15],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [31, 4, 4, 4, 4, 4, 31],
        'J' => [7, 2, 2, 2, 18, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' => [17, 25, 25, 21, 19, 19, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 21, 10],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
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
    fn circle_builder_generates_a_triangle_fan() {
        let mut vertices = Vec::new();
        push_circle(&mut vertices, 0.0, 0.0, 0.1, 0.1, [1.0; 4]);
        assert_eq!(vertices.len(), 24 * 3);
    }

    #[test]
    fn glyph_builder_emits_pixels_for_known_characters() {
        let mut vertices = Vec::new();
        push_text(&mut vertices, "TEXT 42", -0.5, 0.5, 0.02, [1.0; 4]);
        assert!(!vertices.is_empty());
    }

    #[test]
    fn bitmap_font_covers_every_game_status_prompt() {
        for prompt in [
            "SPACE TO LAUNCH",
            "SPACE TO RESTART",
            "VICTORY",
            "GAME OVER",
        ] {
            for character in prompt.chars().filter(|character| *character != ' ') {
                assert!(
                    glyph_rows(character).iter().any(|row| *row != 0),
                    "missing bitmap glyph for {character} in {prompt}"
                );
            }
        }
    }

    #[test]
    fn viewport_uses_the_full_surface_at_the_native_world_aspect_ratio() {
        let viewport = calculate_viewport(960, 720);
        assert_eq!(
            viewport,
            Viewport {
                x: 0.0,
                y: 0.0,
                width: 960.0,
                height: 720.0,
            }
        );
    }

    #[test]
    fn viewport_letterboxes_wide_surfaces_without_stretching_the_world() {
        let viewport = calculate_viewport(1920, 1080);
        assert!((viewport.x - 240.0).abs() < 0.01);
        assert_eq!(viewport.y, 0.0);
        assert!((viewport.width - 1440.0).abs() < 0.01);
        assert_eq!(viewport.height, 1080.0);
    }

    #[test]
    fn viewport_letterboxes_tall_surfaces_without_stretching_the_world() {
        let viewport = calculate_viewport(720, 1280);
        assert_eq!(viewport.x, 0.0);
        assert!((viewport.y - 370.0).abs() < 0.01);
        assert_eq!(viewport.width, 720.0);
        assert!((viewport.height - 540.0).abs() < 0.01);
    }
}
