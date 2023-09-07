// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::{buffer_vec::BufferVec, GlobalRenderResources};
use common::AsBytes;
use std::mem::{self, MaybeUninit};
use ultraviolet::Vec4;

#[derive(Copy, Clone, PartialEq)]
#[repr(C, align(16))]
pub struct BondRepr {
    pub start_pos: Vec4, // with respect to fragment center
    pub end_pos: Vec4,
    pub order: u32,
    #[allow(unused)]
    pub pad: u32,
}

static_assertions::const_assert_eq!(mem::size_of::<BondRepr>(), 48);
unsafe impl AsBytes for BondRepr {}

/// Essentially a per-fragment uniform.
#[repr(C, align(16))]
#[derive(Default)]
pub struct BondBufferHeader;

unsafe impl AsBytes for BondBufferHeader {}

pub struct Bonds {
    bind_group: wgpu::BindGroup,
    buffer: BufferVec<BondBufferHeader, BondRepr>,
    // number_of_bonds: usize,
}

impl Bonds {
    pub fn new<I>(gpu_resources: &GlobalRenderResources, iter: I) -> Self
    where
        I: IntoIterator<Item = BondRepr>,
        I::IntoIter: ExactSizeIterator,
    {
        let bonds = iter.into_iter();
        let number_of_bonds = bonds.len();

        assert!(number_of_bonds > 0, "must have at least one bond");

        let buffer = BufferVec::new_with_data(
            &gpu_resources.device,
            wgpu::BufferUsages::STORAGE,
            number_of_bonds as u64,
            |header, array| {
                // header.write(BondBufferHeader { fragment_id });
                unsafe {
                    std::ptr::write_unaligned(
                        header.as_mut_ptr() as *mut MaybeUninit<BondBufferHeader>,
                        MaybeUninit::new(BondBufferHeader),
                    );
                }

                for (block, bond) in array.iter_mut().zip(bonds) {
                    // block.write(bond);
                    unsafe {
                        std::ptr::write_unaligned(block, MaybeUninit::new(bond));
                    }
                }
            },
        );

        assert!(std::mem::size_of::<BondBufferHeader>() % gpu_resources.device.limits().min_storage_buffer_offset_alignment as usize == 0, "BondBufferHeader's size needs to be an integer multiple of the min storage buffer offset alignment of the gpu. See https://github.com/shinzlet/bondCAD/issues/1");
        let bind_group = gpu_resources
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &gpu_resources.bond_bgl,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: buffer.inner_buffer(),
                        offset: std::mem::size_of::<BondBufferHeader>() as u64,
                        size: None,
                    }),
                }],
            });

        Self {
            bind_group,
            buffer,
            // number_of_bonds,
        }
    }

    pub fn copy_new(&self, render_resources: &GlobalRenderResources) -> Self {
        let buffer = self.buffer.copy_new(render_resources, false);

        render_resources
            .queue
            .write_buffer(buffer.inner_buffer(), 0, BondBufferHeader.as_bytes());

        let bind_group = render_resources
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &render_resources.bond_bgl,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: buffer.inner_buffer(),
                        offset: std::mem::size_of::<BondBufferHeader>() as u64,
                        size: None,
                    }),
                }],
            });

        Self {
            bind_group,
            buffer,
            // number_of_bonds: self.number_of_bonds,
        }
    }

    pub fn bind_group(&self) -> &wgpu::BindGroup {
        &self.bind_group
    }

    pub fn len(&self) -> usize {
        self.buffer.len() as usize
        // self.number_of_bonds
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn with_buffer(&mut self, f: impl Fn(&mut BufferVec<BondBufferHeader, BondRepr>)) {
        f(&mut self.buffer);
    }

    pub fn reupload_bonds(
        &mut self,
        bonds: &[BondRepr],
        gpu_resources: &GlobalRenderResources,
    ) -> crate::buffer_vec::PushStategy {
        let mut encoder = gpu_resources
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        self.buffer.clear();
        println!("buffer size after clear: {}", self.buffer.len());

        let ret = self.buffer.push_small(gpu_resources, &mut encoder, bonds);
        println!("buffer size after push_small: {}", self.buffer.len());
        ret
    }
}

// End of File
