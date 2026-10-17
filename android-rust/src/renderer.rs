use jni::JNIEnv;
use jni::objects::JObject;
use ndk_sys::ANativeWindow_fromSurface;
use raw_window_handle::{AndroidWindowHandle, HasRawWindowHandle};
use wgpu::util::DeviceExt;

// Field order matters: Rust drops fields in declaration order.
// surface must be dropped BEFORE device/queue/instance, so it is declared first.
// instance is declared last so it drops last.
pub struct Renderer {
    surface: wgpu::Surface<'static>,
    queue: wgpu::Queue,
    device: wgpu::Device,
    adapter: wgpu::Adapter,
    instance: wgpu::Instance,
    config: wgpu::SurfaceConfiguration,
    width: u32,
    height: u32,
    _native_window: *mut std::ffi::c_void,
}

impl Renderer {
    pub fn new(env: &JNIEnv, surface_obj: JObject, width: u32, height: u32) -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN | wgpu::Backends::OPENGL,
            ..Default::default()
        });

        // Extract ANativeWindow from Java Surface via JNI
        let env_ptr = env.as_mut_ptr();
        let native_window = unsafe {
            ANativeWindow_fromSurface(env_ptr, surface_obj.into())
        };

        if native_window.is_null() {
            return Err("Failed to create ANativeWindow from Surface".to_string());
        }

        let raw_window_handle = AndroidWindowHandle::new(native_window as *mut std::ffi::c_void);

        let surface = unsafe {
            instance.create_surface(raw_window_handle)
                .map_err(|e| format!("Failed to create wgpu surface: {:?}", e))?
        };

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        })).ok_or_else(|| "Failed to request wgpu adapter".to_string())?;

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                label: None,
            },
            None,
        )).map_err(|e| format!("Failed to create wgpu device: {:?}", e))?;

        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps.formats.iter()
            .copied()
            .find(|f| f.is_srgb())
            .or(surface_caps.formats.first())
            .copied()
            .ok_or_else(|| "No suitable surface format".to_string())?;

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width,
            height,
            present_mode: surface_caps.present_modes[0],
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
        };

        surface.configure(&device, &config);

        Ok(Renderer {
            surface,
            queue,
            device,
            adapter,
            instance,
            config,
            width,
            height,
            _native_window: native_window,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.width = width;
        self.height = height;
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    pub fn render(&mut self) -> Result<(), String> {
        let frame = self.surface.get_current_texture()
            .map_err(|e| format!("Failed to acquire surface texture: {:?}", e))?;

        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("render_encoder"),
        });

        {
            let _rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.1,
                            g: 0.1,
                            b: 0.15,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();

        Ok(())
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        // Fields drop in declaration order: surface first, instance last
    }
}
