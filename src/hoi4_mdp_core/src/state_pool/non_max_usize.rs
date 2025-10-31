/// A `usize` that can never be `usize::MAX`, similar to `NonZeroUsize` but excludes `usize::MAX` instead of `0`.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NonMaxUsize(usize);

impl NonMaxUsize {
    #[inline]
    pub fn new(value: usize) -> Option<Self> {
        if value == usize::MAX { None } else { Some(unsafe { Self::new_unchecked(value) }) }
    }
    #[inline]
    pub unsafe fn new_unchecked(value: usize) -> Self {
        debug_assert_ne!(value, usize::MAX, "NonMaxUsize cannot be usize::MAX");
        NonMaxUsize(value)
    }
    #[inline]
    pub fn get(self) -> usize { self.0 }
}

impl From<NonMaxUsize> for usize {
    fn from(val: NonMaxUsize) -> Self { val.get() }
}
