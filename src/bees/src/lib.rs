use std::{num::NonZeroU64, ptr::NonNull};

// === WideRef === //

pub struct WideRef<T: ?Sized> {
    gen: NonZeroU64,
    slot: NonNull<u64>,
    data: NonNull<T>,
}

impl<T: ?Sized> WideRef<T> {
    const DANGLING_ERR: &str = "attempted to deref a dead pointer";

    #[inline(always)]
    pub fn is_alive(self) -> bool {
        unsafe { *self.slot.as_ptr() == self.gen.get() }
    }

    #[inline(always)]
    pub fn try_get(self) -> Option<NonNull<T>> {
        if self.is_alive() {
            Some(self.data)
        } else {
            None
        }
    }

    #[inline(always)]
    pub fn get(self) -> NonNull<T> {
        self.try_get().expect(Self::DANGLING_ERR)
    }

    #[inline(always)]
    pub fn get_unchecked(self) -> NonNull<T> {
        self.data
    }

    #[inline(always)]
    pub fn try_read(self) -> Option<T>
    where
        T: Copy,
    {
        if let Some(ptr) = self.try_get() {
            Some(unsafe { ptr.as_ptr().read() })
        } else {
            None
        }
    }

    #[inline(always)]
    pub fn read(self) -> T
    where
        T: Copy,
    {
        self.try_read().expect(Self::DANGLING_ERR)
    }

    #[inline(always)]
    pub fn try_write(self, value: T) -> Option<T>
    where
        T: Sized,
    {
        if let Some(ptr) = self.try_get() {
            let read = unsafe { ptr.as_ptr().read() };
            unsafe { ptr.as_ptr().write(value) };
            Some(read)
        } else {
            None
        }
    }

    #[inline(always)]
    pub fn write(self, value: T) -> T
    where
        T: Sized,
    {
        self.try_write(value).expect(Self::DANGLING_ERR)
    }

    #[inline(always)]
    pub fn try_take(self) -> Option<T>
    where
        T: Sized,
    {
        if let Some(ptr) = self.try_get() {
            unsafe { *self.slot.as_ptr() = 0 };
            Some(unsafe { ptr.as_ptr().read() })
        } else {
            None
        }
    }

    #[inline(always)]
    pub unsafe fn subfield_unchecked<U: ?Sized>(self, data: NonNull<U>) -> WideRef<U> {
        WideRef {
            gen: self.gen,
            slot: self.slot,
            data,
        }
    }

    #[inline(always)]
    pub fn get_for_macro(self, _: FuncDisambiguator) -> (Self, NonNull<T>) {
        (self, self.get())
    }

    #[must_use]
    pub fn try_destroy(self) -> bool {
        if let Some(ptr) = self.try_get() {
            unsafe { ptr.as_ptr().drop_in_place() };
            true
        } else {
            false
        }
    }

    #[inline(always)]
    pub fn destroy(self) {
        assert!(self.try_destroy(), "{}", Self::DANGLING_ERR);
    }
}

impl<T: ?Sized> Copy for WideRef<T> {}

impl<T: ?Sized> Clone for WideRef<T> {
    fn clone(&self) -> Self {
        *self
    }
}

#[macro_export]
macro_rules! subfield {
    ($target:expr, $field:ident) => {{
        let (target, ptr) =
            $target.get_for_macro($crate::subfield_internals::get_func_disambiguator());

        let ptr = ptr.as_ptr();
        let ptr = unsafe { $crate::subfield_internals::addr_of_mut!((*ptr).$field) };

        unsafe {
            target.subfield_unchecked(
                $crate::subfield_internals::NonNull::new(ptr).unwrap_unchecked(),
            )
        }
    }};
}

#[doc(hidden)]
pub mod subfield_internals {
    use super::*;

    pub use std::ptr::{addr_of_mut, NonNull};

    #[inline(always)]
    pub fn get_func_disambiguator() -> FuncDisambiguator {
        FuncDisambiguator
    }
}

mod func_disambiguator_sealed {
    pub struct FuncDisambiguator;
}

pub(crate) use func_disambiguator_sealed::FuncDisambiguator;

// === Struct === //

pub trait Struct {
    type WideWrapper: WideWrapper;
}

pub trait WideWrapper: Copy {
    type Pointee: ?Sized;

    fn from_raw(raw: WideRef<Self::Pointee>) -> Self;

    fn raw(self) -> WideRef<Self::Pointee>;
}

// === Macros === //

pub extern crate self as bees;

pub use bees_macro::Struct;

#[doc(hidden)]
pub mod derive_struct_internal {
    pub use {Clone, Copy};

    pub trait TrivialBound<'__> {
        type Itself: ?Sized;
    }

    impl<T: ?Sized> TrivialBound<'_> for T {
        type Itself = Self;
    }
}
