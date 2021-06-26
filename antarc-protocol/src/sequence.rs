use std::num::NonZeroU32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Sequence(pub NonZeroU32);

impl Sequence {
    pub const fn one() -> Self {
        unsafe { Self(NonZeroU32::new_unchecked(1)) }
    }

    pub fn new(non_zero_value: u32) -> Option<Self> {
        NonZeroU32::new(non_zero_value).map(|non_zero| Sequence(non_zero))
    }

    pub unsafe fn new_unchecked(non_zero_value: u32) -> Self {
        Self(NonZeroU32::new_unchecked(non_zero_value))
    }

    pub fn get(&self) -> u32 {
        self.0.get()
    }
}

impl Default for Sequence {
    fn default() -> Self {
        unsafe { Self::new_unchecked(1) }
    }
}
