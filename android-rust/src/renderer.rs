use jni::JNIEnv;
use jni::objects::JObject;
use std::ptr::null_mut;
use wgpu::{
    Backends, Instance, InstanceDescriptor, RequestAdapterOptions, Surface, SurfaceConfiguration,
    SurfaceTarget,
};

/// Wrapper around ANativeWindow pointer to make it Send + Sync.
/// Raw pointers are always Send + Sync, but the compiler sometimes struggles
/// with opaque FFI types.
struct AndroidWindow(*mut ndk_sys::ANativeWindow);

unsafe impl Send for AndroidWindow {}
unsafe impl Sync for AndroidWindow {}

/// Holds the wgpu device, queue, and surface tied to an Android SurfaceView.
pub struct Renderer {
    _instance: Instance,
    _adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface: Surface<'static>,
    pub config: SurfaceConfiguration,
    /// Keep the ANativeWindow alive for the lifetime of the surface.
    _window: AndroidWindow,
}

impl Renderer {
    pub fn new(env: &JNIEnv, surface: JObject, width: u32, height: u32) -> Result<Self, String> {
        // Extract the raw ANativeWindow pointer from the Java Surface object.
        let window = unsafe {
            ndk_sys::ANativeWindow_from_surface(
                env.get_raw_jnienv() as *mut _,
                surface.into_raw() as *mut _,
            )
        };
        if window.is_null() {
            return Err("Failed to get ANativeWindow from Java Surface".to_string());
        }

        let instance = Instance::new(InstanceDescriptor {
            backends: Backends::VULKAN.primary(),
            ..Default::default()
        });

        let surface = unsafe {
            instance.create_surface(SurfaceTarget::AndroidNdk(window as *mut _))
                .map_err(|e| format!("Failed to create wgpu surface: {e}"))?
        };

        let adapter = futures_util::executor::block_on(instance.request_adapter(&RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .ok_or_else(|| "No wgpu adapter found".to_string())?;

        let (device, queue) = futures_util::executor::block_on(
            adapter.request_device(&wgpu::DeviceDescriptor::default(), None),
        )
        .map_err(|e| format!("Failed to create wgpu device: {e}"))?;

        let config = surface
            .get_default_config(&adapter, width, height)
            .ok_or_else(|| "Failed to get default surface config".to_string())?;
        surface.configure(&device, &config);

        Ok(Renderer {
            _instance: instance,
            _adapter: adapter,
            device,
            queue,
            surface,
            config,
            _window: AndroidWindow(window),
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    pub fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        let frame = self.surface.get_current_texture()?;
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self.device.create_command_encoder(
            &wgpu::CommandEncoderDescriptor {
                label: Some("veloren_frame"),
            },
        );

        {
            let mut render_pass = encoder.begin_render_pass(
                &wgpu::RenderPassDescriptor {
                    label: Some("veloren_render_pass"),
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
                },
            );
            // No draw calls yet — clear screen only. Veloren voxygen renderer integrates here.
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();

        Ok(())
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        if !self._window.0.is_null() {
            unsafe {
                ndk_sys::ANativeWindow_release(self._window.0);
            }
        }
    }
}