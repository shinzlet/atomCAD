use std::{
    ops::Range,
    mem,
    slice,
};
use ultraviolet::Vec3;

pub struct BoundingBox {
    pub min: Vec3,
    pub max: Vec3,
}

impl BoundingBox {
    pub fn union(&self, other: &Self) -> Self {
        Self {
            min: Vec3::new(
                self.min.x.min(other.min.x),
                self.min.y.min(other.min.y),
                self.min.z.min(other.min.z),
            ),
            max: Vec3::new(
                self.max.x.max(other.max.x),
                self.max.y.max(other.max.y),
                self.max.z.max(other.max.z),
            )
        }
    }

    pub fn contains(&self, point: Vec3) -> bool {
        self.min.x <= point.x && point.x <= self.max.x
        && self.min.y <= point.y && point.y <= self.max.y
        && self.min.z <= point.z && point.z <= self.max.z
    }
}

pub unsafe trait AsBytes {
    fn as_bytes(&self) -> &[u8] {
        unsafe {
            slice::from_raw_parts(self as *const _ as *const u8, mem::size_of_val(self))
        }
    }
}

unsafe impl<T> AsBytes for [T] where T: AsBytes + Sized {
    fn as_bytes(&self) -> &[u8] {
        unsafe {
            slice::from_raw_parts(self.as_ptr().cast(), mem::size_of::<T>() * self.len())
        }
    }
}
