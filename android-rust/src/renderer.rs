use jni::JNIEnv;
use jni::objects::JObject;
use ndk_sys::ANativeWindow_fromSurface;
use raw_window_handle::{
    AndroidDisplayHandle, AndroidNdkWindowHandle, RawDisplayHandle, RawWindowHandle,
};
use wgpu::util::DeviceExt;

/// The `ANativeWindow` reference acquired by `ANativeWindow_fromSurface`.
/// Releasing it on drop keeps the reference count balanced; the field order
/// of `Renderer` makes this happen only after the wgpu surface is gone.
struct NativeWindowRef(*mut std::ffi::c_void);

impl Drop for NativeWindowRef {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { ndk_sys::ANativeWindow_release(self.0.cast()) };
        }
    }
}

// Field order matters: Rust drops fields in declaration order.
// surface must be dropped BEFORE device/queue/instance, so it is declared first.
// instance is declared next-to-last, and the native window reference (declared
// last) is released only once the surface using it has been dropped.
pub struct Renderer {
    surface: wgpu::Surface<'static>,
    queue: wgpu::Queue,
    device: wgpu::Device,
    adapter: wgpu::Adapter,
    instance: wgpu::Instance,
    config: wgpu::SurfaceConfiguration,
    width: u32,
    height: u32,
    _native_window: NativeWindowRef,
}

impl Renderer {
    pub fn new(env: &JNIEnv, surface_obj: JObject, width: u32, height: u32) -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..Default::default()
        });

        // jni 0.21 and ndk-sys 0.6 both bind jni-sys 0.3, so the raw pointers line up.
        let native_window = unsafe {
            ANativeWindow_fromSurface(env.get_raw(), surface_obj.into_raw())
        };
        // Wrapped immediately: every early return below releases the reference.
        let native_window_ref = NativeWindowRef(native_window as *mut std::ffi::c_void);

        let native_window_ptr = std::ptr::NonNull::new(native_window as *mut std::ffi::c_void)
            .ok_or_else(|| "Failed to create ANativeWindow from Surface".to_string())?;

        let raw_window_handle = AndroidNdkWindowHandle::new(native_window_ptr);

        // raw-window-handle 0.6 has no HasWindowHandle impl for AndroidNdkWindowHandle,
        // so the surface has to be created through the unsafe raw-handle path.
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: RawDisplayHandle::Android(AndroidDisplayHandle::new()),
                raw_window_handle: RawWindowHandle::AndroidNdk(raw_window_handle),
            })
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
                memory_hints: wgpu::MemoryHints::default(),
            },
            None,
        )).map_err(|e| format!("Failed to create wgpu device: {:?}", e))?;

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps.formats.iter()
            .copied()
            .find(|f| f.is_srgb())
            .or_else(|| surface_caps.formats.first().copied())
            .ok_or_else(|| "No suitable surface format".to_string())?;

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width,
            height,
            present_mode: surface_caps.present_modes[0],
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
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
            _native_window: native_window_ref,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        // A window that was destroyed reports no supported present modes and
        // configuring such a surface panics inside wgpu; skip until a fresh
        // renderer is created for the new window instead.
        let Some(present_mode) = self
            .surface
            .get_capabilities(&self.adapter)
            .present_modes
            .first()
            .copied()
        else {
            return;
        };
        self.width = width;
        self.height = height;
        self.config.width = width;
        self.config.height = height;
        self.config.present_mode = present_mode;
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
        // Fields drop in declaration order: surface first, native window last
    }
}
