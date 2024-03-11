use wgpu::{include_wgsl, util::DeviceExt, BufferAsyncError};
use winit::{event::{ElementState, KeyEvent, WindowEvent}, keyboard::{Key, NamedKey}, window::Window};

#[repr(C)]
#[derive(PartialEq, Eq, Hash, Debug, Default, Copy, Clone)]
pub enum VoxelState {
    #[default]
    Air = 0,
    Sand
}

const CHUNK_HORIZONTAL: usize = 4;
const CHUNK_VERTICAL: usize = 4;

#[repr(C, align(16))]
#[derive(PartialEq, Eq, Hash, Debug, Default, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Chunk {
    pub voxels: [[[u8; CHUNK_HORIZONTAL]; CHUNK_HORIZONTAL]; CHUNK_VERTICAL]
}

pub struct State<'window> {
    pub surface: wgpu::Surface<'window>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    pub win_size: winit::dpi::PhysicalSize<u32>,
    pub render_pipeline: wgpu::RenderPipeline,
    pub compute: Compute,
}

pub struct Compute {
    pub pipeline: wgpu::ComputePipeline,
    pub staging_buffer: wgpu::Buffer,
    pub pipe: (flume::Sender<Result<(), BufferAsyncError>>, flume::Receiver<Result<(), BufferAsyncError>>),
}

impl<'window> State<'window> {
    // Creating some of the wgpu types requires async code
    pub async fn new(window: &'window Window) -> Self {
        let win_size = window.inner_size();

        // The instance is a handle to our GPU
        // Backends::all => Vulkan + Metal + DX12 + Browser WebGPU
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        // # Safety
        //
        // The surface needs to live as long as the window that created it.
        // State owns the window, so this should be safe.
        let surface = instance.create_surface(window).unwrap();

        let adapter = instance.request_adapter(
            &wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            },
        ).await.unwrap();

        let (device, queue) = adapter.request_device(
            &wgpu::DeviceDescriptor {
                required_features: wgpu::Features::empty(),
                // WebGL doesn't support all of wgpu's features, so if
                // we're building for the web, we'll have to disable some.
                required_limits: if cfg!(target_arch = "wasm32") {
                    wgpu::Limits::downlevel_webgl2_defaults()
                } else {
                    wgpu::Limits::default()
                },
                label: None,
            },
            None, // Trace path
        ).await.unwrap();

        let surface_caps = surface.get_capabilities(&adapter);
        // Shader code in this tutorial assumes an sRGB surface texture. Using a different
        // one will result in all the colors coming out darker. If you want to support non
        // sRGB surfaces, you'll need to account for that when drawing to the frame.
        let surface_format = surface_caps.formats.iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: win_size.width,
            height: win_size.height,
            present_mode: surface_caps.present_modes[0],
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 0
        };
        surface.configure(&device, &config);

        let shader = device.create_shader_module(include_wgsl!("shader.wgsl"));

        let render_pipeline_layout =
    device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Render Pipeline Layout"),
        bind_group_layouts: &[],
        push_constant_ranges: &[],
    });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent::REPLACE,
                        alpha: wgpu::BlendComponent::REPLACE,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                // Setting this to anything other than Fill requires Features::POLYGON_MODE_LINE
                // or Features::POLYGON_MODE_POINT
                polygon_mode: wgpu::PolygonMode::Fill,
                // Requires Features::DEPTH_CLIP_CONTROL
                unclipped_depth: false,
                // Requires Features::CONSERVATIVE_RASTERIZATION
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            // If the pipeline will be used with a multiview render pass, this
            // indicates how many array layers the attachments will have.
            multiview: None,
        });


        Self {
            surface,
            queue,
            config,
            win_size,
            render_pipeline,
            compute: {
                let compute_shader = device.create_shader_module(include_wgsl!("compute.wgsl"));

                // A bind group defines how buffers are accessed by shaders.
                // It is to WebGPU what a descriptor set is to Vulkan.
                // `binding` here refers to the `binding` of a buffer in the shader (`layout(set = 0, binding = 0) buffer`).

                // A pipeline specifies the operation of a shader

                // Instantiates the pipeline.
                let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: None,
                    layout: None,
                    module: &compute_shader,
                    entry_point: "main",
                });

                // Instantiates buffer without data.
                // `usage` of buffer specifies how it can be used:
                //   `BufferUsages::MAP_READ` allows it to be read (outside the shader).
                //   `BufferUsages::COPY_DST` allows it to be the destination of the copy.
                let staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("CPU staging buffer"),
                    size: std::mem::size_of::<Chunk>() as u64,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });

                Compute {
                    pipeline,
                    staging_buffer,
                    pipe: flume::bounded(1)
                }
            },
            device,
        }
    }

    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.win_size = new_size;
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    pub fn input(&mut self, event: &WindowEvent) -> bool {
        match event {
            WindowEvent::KeyboardInput { event: KeyEvent { state: ElementState::Pressed, logical_key, .. }, .. } => match logical_key.as_ref() {
                Key::Named(NamedKey::F11) => {
                    eprintln!("F11");
                    true
                },
                Key::Character("a") => true,
                _ => false,
            },
            _ => false,
        }
    }

    pub fn update(&mut self) {
    }

    pub fn start_compute(&self, chunk: &Chunk) {
        let workgroups = 1;


        // Instantiates buffer with data (`numbers`).
        // Usage allowing the buffer to be:
        //   A storage buffer (can be bound within a bind group and thus available to a shader).
        //   The destination of a copy.
        //   The source of a copy.
        let storage_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Storage Buffer"),
            //contents: bytemuck::cast_slice(data),
            contents: (bytemuck::cast_ref::<_, [u8; std::mem::size_of::<Chunk>()]>(chunk)),
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
        });

        // Instantiates the bind group, once again specifying the binding of buffers.
        let bind_group_layout = self.compute.pipeline.get_bind_group_layout(0);
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: storage_buffer.as_entire_binding(),
            }],
        });

        // A command encoder executes one or many pipelines.
        // It is to WebGPU what a command buffer is to Vulkan.
        let mut encoder =
            self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: None,
                timestamp_writes: None,
            });
            cpass.set_pipeline(&self.compute.pipeline);
            cpass.set_bind_group(0, &bind_group, &[]);
            cpass.insert_debug_marker("compute collatz iterations");
            cpass.dispatch_workgroups(workgroups as u32, 1, 1); // Number of cells to run, the (x,y,z) size of item being processed
        }
        // Sets adds copy operation to command encoder.
        // Will copy data from storage buffer on GPU to staging buffer on CPU.
        encoder.copy_buffer_to_buffer(&storage_buffer, 0, &self.compute.staging_buffer, 0, storage_buffer.size());

        // Submits command encoder for processing
        self.queue.submit(Some(encoder.finish()));

        // Note that we're not calling `.await` here.
        let buffer_slice = self.compute.staging_buffer.slice(..);
        // Sets the buffer up for mapping, sending over the result of the mapping back to us when it is finished.
        let sender = self.compute.pipe.0.clone();
        buffer_slice.map_async(wgpu::MapMode::Read, move |v| sender.send(v).unwrap());
    }

    pub fn recv_compute(&self) -> Option<Chunk> {
        match self.compute.pipe.1.try_recv() {
            Ok(Ok(())) => {
                // Gets contents of buffer
                let result = {
                    let buffer_slice = self.compute.staging_buffer.slice(..);
                    let data = buffer_slice.get_mapped_range();
                    let data: [u8; std::mem::size_of::<Chunk>()] = (&*data).try_into().expect("who placed invalid data onto the gpu buffer!!!!!");
                    bytemuck::cast(data)
                };

                self.compute.staging_buffer.unmap();

                // Returns data from buffer
                Some(result)
            }
            Ok(Err(e)) => {
                eprintln!("buffer error: {e}");
                None
            }
            Err(_) => {
                None
            }
        }
    }
    pub fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        let output = self.surface.get_current_texture()?;
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Render Encoder"),
        });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.1,
                            g: 0.2,
                            b: 0.3,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });

            render_pass.set_pipeline(&self.render_pipeline);
            render_pass.draw(0..3, 0..1);
        }

        // submit will accept anything that implements IntoIter
        self.queue.submit(Some(encoder.finish()));
        output.present();

        Ok(())
    }
}
