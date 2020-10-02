use crate::{buffer_vec::BufferVec, GlobalGpuResources};
use common::AsBytes;
use periodic_table::Element;
use std::mem;
use ultraviolet::Vec3;

/// Packed bit field
/// | 0 .. 7 | ----------- | 7 .. 31 |
///   ^ atomic number - 1    ^ unspecified
///
/// TODO: Try using a buffer as an atom radius lookup table.
#[derive(Copy, Clone, PartialEq, Eq)]
#[repr(transparent)]
pub struct AtomKind(u32);
impl AtomKind {
    pub fn new(element: Element) -> Self {
        Self(((element as u8 - 1) & 0b1111_111) as u32)
    }

    pub fn element(&self) -> Element {
        let n = (self.0 & 0b1111_111) as u8 + 1;
        Element::from_atomic_number(n)
            .unwrap_or_else(|| unreachable!("invalid atomic number in atom kind"))
    }
}

#[derive(Copy, Clone, PartialEq)]
#[repr(C)]
pub struct AtomRepr {
    pub pos: Vec3, // with respect to fragment center
    pub kind: AtomKind,
}

static_assertions::const_assert_eq!(mem::size_of::<AtomRepr>(), 16);
unsafe impl AsBytes for AtomRepr {}

pub struct Atoms {
    bind_group: wgpu::BindGroup,
    buffer: BufferVec,
    number_of_atoms: usize,
}

impl Atoms {
    pub fn new<I>(gpu_resources: &GlobalGpuResources, iter: I) -> Self
    where
        I: IntoIterator<Item = AtomRepr>,
        I::IntoIter: ExactSizeIterator,
    {
        let atoms = iter.into_iter();
        let number_of_atoms = atoms.len();

        assert!(number_of_atoms > 0, "must have at least one atom");

        let buffer = BufferVec::new_with_data(
            &gpu_resources.device,
            wgpu::BufferUsage::STORAGE,
            (number_of_atoms * mem::size_of::<AtomRepr>()) as u64,
            |buffer_view| {
                buffer_view
                    .chunks_exact_mut(mem::size_of::<AtomRepr>())
                    .zip(atoms)
                    .for_each(|(chunk, atom)| {
                        chunk.copy_from_slice(atom.as_bytes());
                    });
            },
        );

        let bind_group = gpu_resources
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &gpu_resources.bgl.atoms,
                entries: &[wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Buffer {
                        buffer: buffer.inner_buffer(),
                        offset: 0,
                        size: None,
                    },
                }],
            });

        Self {
            bind_group,
            buffer,
            number_of_atoms,
        }
    }

    pub fn bind_group(&self) -> &wgpu::BindGroup {
        &self.bind_group
    }

    pub fn len(&self) -> usize {
        self.number_of_atoms
    }
}
