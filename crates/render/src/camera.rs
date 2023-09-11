// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::bind_groups::AsBindingResource;
use common::{AsBytes, InputEvent};
use std::mem;
use ultraviolet::{Mat4, Vec3};
use winit::dpi::{PhysicalPosition, PhysicalSize};

#[derive(Clone, Default)]
#[repr(C)]
pub struct CameraRepr {
    pub projection: Mat4,
    pub view: Mat4,
    pub projection_view: Mat4,
}

unsafe impl AsBytes for CameraRepr {}

pub trait Camera {
    fn resize(&mut self, aspect: f32, fov: f32, near: f32);
    fn update(&mut self, event: InputEvent) -> bool;
    fn finalize(&mut self);
    fn repr(&self) -> CameraRepr;
    fn position(&self) -> Vec3;
}

pub struct RenderCamera {
    // Things related to on-gpu representation.
    uniform_buffer: wgpu::Buffer,

    fov: f32,
    near: f32,
    camera: Option<Box<dyn Camera>>,
    camera_was_updated: bool,
}

impl RenderCamera {
    pub(crate) fn new_empty(device: &wgpu::Device, fov: f32, near: f32) -> Self {
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: mem::size_of::<CameraRepr>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            uniform_buffer,

            fov,
            near,
            camera: None,
            camera_was_updated: false,
        }
    }

    pub(crate) fn set_camera<C: Camera + 'static>(
        &mut self,
        mut camera: C,
        size: PhysicalSize<u32>,
    ) {
        camera.resize(size.width as f32 / size.height as f32, self.fov, self.near);
        self.camera = Some(Box::new(camera));
        self.camera_was_updated = true;
    }

    pub fn set_fov(&mut self, fov: f32) {
        self.fov = fov;
        self.camera_was_updated = true;
    }

    pub fn set_near(&mut self, near: f32) {
        self.near = near;
        self.camera_was_updated = true;
    }

    pub fn resize(&mut self, new_size: PhysicalSize<u32>) {
        if let Some(camera) = self.camera.as_mut() {
            camera.resize(
                new_size.width as f32 / new_size.height as f32,
                self.fov,
                self.near,
            );
            self.camera_was_updated = true;
        }
        // let mut camera = self.camera;
        // camera.as_mut().map(|camera| {
        //     camera.resize(new_size.width as f32 / new_size.height as f32, self.fov, self.near);
        //     self.camera_was_updated = true;
        // });
        // self.camera_impl
        //     .resize(new_size.width as f32 / new_size.height as f32, self.fov, self.near);
    }

    pub fn update(&mut self, event: InputEvent) {
        if let Some(camera) = self.camera.as_mut() {
            self.camera_was_updated |= camera.update(event);
        }
        // self.camera.map(|camera| {
        //     self.camera_was_updated |= camera.update(event);
        // });
    }

    #[must_use = "returns bool indicating whether a camera is currently set or not"]
    pub(crate) fn upload(&mut self, queue: &wgpu::Queue) -> bool {
        if let Some(camera) = self.camera.as_mut() {
            camera.finalize();
            if self.camera_was_updated {
                queue.write_buffer(&self.uniform_buffer, 0, camera.repr().as_bytes());
            }
            self.camera_was_updated = false;
        }
        self.camera.is_some()

        // self.camera.map(|camera| {
        //     camera.finalize();
        //     if self.camera_was_updated {
        //         queue.write_buffer(&self.uniform_buffer, 0, camera.repr().as_bytes());
        //     }
        //     self.camera_was_updated = false;
        // }).is_some()
        // self.camera.map(|camera| camera.finalize());
        // if self.camera_was_updated {
        //     self.camera_was_updated = false;
        //     queue.write_buffer(&self.uniform_buffer, 0, self.camera_impl.repr().as_bytes());
        // }
    }

    pub fn get_ray_from(
        &self,
        pixel: &PhysicalPosition<f64>,
        viewport_size: &PhysicalSize<u32>,
    ) -> Option<(Vec3, Vec3)> {
        let camera = self.camera.as_ref()?;
        let camera_repr = camera.repr();

        // 1. Convert the pixel position to normalized device coordinates.
        let x = (2.0 * pixel.x as f32 - viewport_size.width as f32) / viewport_size.width as f32;
        let y = (viewport_size.height as f32 - 2.0 * pixel.y as f32) / viewport_size.height as f32;

        // 2. Create a ray in clip space.
        let ray_clip = Vec3::new(x, y, -self.near);

        // 3. Inverse project this ray from clip space to camera's view space.
        let proj_inv = camera_repr.projection.inversed();
        let ray_eye = proj_inv.transform_vec3(ray_clip);

        // For the perspective projection, we need to flip the direction along the z-axis
        let ray_eye = Vec3::new(ray_eye.x, ray_eye.y, -1.0);

        // 4. Inverse transform this ray from the camera's view space to world space.
        let view_inv = camera_repr.view.inversed();
        let ray_world = view_inv.transform_vec3(ray_eye);

        // Normalize the ray's direction
        let ray_dir = ray_world.normalized();

        Some((camera.position(), ray_dir))
    }
}

impl AsBindingResource for RenderCamera {
    fn as_binding_resource(&self) -> wgpu::BindingResource {
        wgpu::BindingResource::Buffer(wgpu::BufferBinding {
            buffer: &self.uniform_buffer,
            offset: 0,
            size: None,
        })
    }
}

// End of File
