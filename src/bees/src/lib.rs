use std::{
    cell::{Cell, UnsafeCell},
    mem::MaybeUninit,
    num::NonZeroU64,
    sync::atomic::{AtomicU64, Ordering::Relaxed},
};

// === Arena === //

fn gen() -> NonZeroU64 {
    static GEN: AtomicU64 = AtomicU64::new(1);
    NonZeroU64::new(GEN.fetch_add(1, Relaxed)).unwrap()
}

pub struct Allocation<T: 'static> {
    values: &'static [Generational<T>],
}

impl<T> Allocation<T> {
    pub fn new(len: usize) -> Self {
        Self {
            values: Box::leak(Box::from_iter((0..len).map(|_| Generational::new_empty()))),
        }
    }

    pub fn put_with_gen(&self, index: usize, gen: NonZeroU64, value: T) -> Ref<T> {
        let slot = &self.values[index];
        unsafe { slot.replace(Some((gen, value))) };

        Ref {
            gen,
            gen_ptr: &slot.gen,
            value: slot.get(),
        }
    }

    pub fn put(&self, index: usize, value: T) -> Ref<T> {
        self.put_with_gen(index, gen(), value)
    }

    pub fn try_get(&self, index: usize) -> Option<Ref<T>> {
        let slot = &self.values[index];

        if slot.is_full() {
            Some(Ref {
                gen: NonZeroU64::new(slot.gen.get()).unwrap(),
                gen_ptr: &slot.gen,
                value: slot.get(),
            })
        } else {
            None
        }
    }

    pub fn get(&self, index: usize) -> Ref<T> {
        self.try_get(index).unwrap()
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }
}

struct Generational<T> {
    gen: Cell<u64>,
    value: UnsafeCell<MaybeUninit<T>>,
}

impl<T> Generational<T> {
    pub fn new(value: Option<(NonZeroU64, T)>) -> Self {
        match value {
            Some((gen, value)) => Self::new_full(gen, value),
            None => Self::new_empty(),
        }
    }

    pub const fn new_empty() -> Self {
        Self {
            gen: Cell::new(0),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    pub const fn new_full(gen: NonZeroU64, value: T) -> Self {
        Self {
            gen: Cell::new(gen.get()),
            value: UnsafeCell::new(MaybeUninit::new(value)),
        }
    }

    pub fn is_full(&self) -> bool {
        self.gen.get() != 0
    }

    pub fn get(&self) -> *mut T {
        self.value.get() as *mut T
    }

    pub fn get_mut(&mut self) -> Option<&mut T> {
        if self.is_full() {
            Some(unsafe { self.value.get_mut().assume_init_mut() })
        } else {
            None
        }
    }

    pub unsafe fn replace(&self, value: Option<(NonZeroU64, T)>) -> Option<T> {
        let old = if self.is_full() {
            Some(unsafe { self.get().read() })
        } else {
            None
        };

        if let Some((gen, value)) = value {
            self.gen.set(gen.get());
            self.get().write(value);
        }

        old
    }
}

impl<T> Drop for Generational<T> {
    fn drop(&mut self) {
        if self.is_full() {}
    }
}

// === Ref === //

const DANGLING_ERR: &str = "attempted to deref a dead pointer";

pub struct Ref<T: 'static> {
    gen_ptr: &'static Cell<u64>,
    gen: NonZeroU64,
    value: *mut T,
}

impl<T> Ref<T> {
    #[inline(always)]
    pub fn is_alive(self) -> bool {
        self.gen.get() == self.gen_ptr.get()
    }

    #[inline(always)]
    pub fn get_unchecked(self) -> *mut T {
        self.value
    }

    #[inline(always)]
    pub fn try_get(self) -> Option<*mut T> {
        if self.is_alive() {
            Some(self.get_unchecked())
        } else {
            None
        }
    }

    #[inline(always)]
    pub fn get(self) -> *mut T {
        self.try_get().expect(DANGLING_ERR)
    }

    #[inline(always)]
    pub fn try_read(self) -> Option<T>
    where
        T: Copy,
    {
        if let Some(ptr) = self.try_get() {
            Some(unsafe { ptr.read() })
        } else {
            None
        }
    }

    #[inline(always)]
    pub fn read(self) -> T
    where
        T: Copy,
    {
        self.try_read().expect(DANGLING_ERR)
    }

    #[inline(always)]
    pub fn try_write(self, value: T) -> Option<T>
    where
        T: Sized,
    {
        if let Some(ptr) = self.try_get() {
            let read = unsafe { ptr.read() };
            unsafe { ptr.write(value) };
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
        self.try_write(value).expect(DANGLING_ERR)
    }

    #[inline(always)]
    pub unsafe fn subfield_unchecked<U>(self, data: *mut U) -> Ref<U> {
        Ref {
            gen_ptr: self.gen_ptr,
            gen: self.gen,
            value: data,
        }
    }

    #[inline(always)]
    pub fn get_for_macro(self, _: FuncDisambiguator) -> (Self, *mut T) {
        (self, self.get())
    }

    pub fn wrap(self) -> T::Wrapper
    where
        T: Struct,
    {
        RefWrapper::from_raw(self)
    }
}

impl<T> Copy for Ref<T> {}

impl<T> Clone for Ref<T> {
    fn clone(&self) -> Self {
        *self
    }
}

#[macro_export]
macro_rules! subfield {
    ($target:expr, $field:ident) => {{
        let (target, ptr) =
            $target.get_for_macro($crate::subfield_internals::get_func_disambiguator());

        let ptr = unsafe {
            // Safety: this is a valid pointer to some data.
            $crate::subfield_internals::addr_of_mut!((*ptr).$field)
        };

        unsafe {
            // Safety: this field will not expire until the parent structure has expired.
            target.subfield_unchecked(ptr)
        }
    }};
}

#[doc(hidden)]
pub mod subfield_internals {
    use super::*;

    pub use std::ptr::addr_of_mut;

    #[inline(always)]
    pub fn get_func_disambiguator() -> FuncDisambiguator {
        FuncDisambiguator
    }
}

mod func_disambiguator_sealed {
    pub struct FuncDisambiguator;
}

pub(crate) use func_disambiguator_sealed::FuncDisambiguator;

// === MovableRef === //

pub struct MovableRef<T> {
    gen_ptr: &'static Cell<u64>,
    gen: NonZeroU64,
    value: Cell<*mut T>,
}

// TODO: Implement `MovableRef`

// === ThinRef === //

// TODO: Implement `ThinRef`

// === Struct === //

pub trait Struct {
    type Wrapper: RefWrapper<Pointee = Self>;
}

pub trait RefWrapper: Copy {
    type Pointee;

    fn from_raw(raw: Ref<Self::Pointee>) -> Self;

    fn raw(self) -> Ref<Self::Pointee>;
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
