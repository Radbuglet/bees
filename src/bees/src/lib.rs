use std::{
    cell::{Cell, RefCell},
    marker::PhantomData,
    ptr::null_mut,
};

// === Slot Manager === //

struct Slot {
    gen: Cell<u64>,
    data: Cell<*mut ()>,
}

thread_local! {
    static FREE_SLOTS: RefCell<Vec<&'static Slot>> = RefCell::new(Vec::new());
}

fn alloc_slot() -> &'static Slot {
    FREE_SLOTS.with(|slots| {
        let slots = &mut *slots.borrow_mut();

        if let Some(slot) = slots.pop() {
            slot
        } else {
            slots.extend(
                Box::leak(Box::from_iter((0..128).map(|_| Slot {
                    gen: Cell::new(0),
                    data: Cell::new(null_mut()),
                })))
                .iter(),
            );

            slots.pop().unwrap()
        }
    })
}

fn dealloc_slot(slot: &'static Slot) {
    FREE_SLOTS.with(|slots| slots.borrow_mut().push(slot));
}

// === Ref === //

const DANGLING_ERR: &str = "attempted to deref a dead pointer";

pub struct Ref<T> {
    _ty: PhantomData<*const T>,
    gen: u64,
    slot: &'static Slot,
    offset: usize,
}

impl<T> Ref<T> {
    pub fn new(value: T) -> Self {
        let value = Box::new(value);
        let slot = alloc_slot();
        slot.gen.set(slot.gen.get() + 1);
        slot.data.set(Box::leak(value) as *mut T as *mut ());

        Self {
            _ty: PhantomData,
            gen: slot.gen.get(),
            slot,
            offset: 0,
        }
    }

    #[inline(always)]
    pub fn is_alive(self) -> bool {
        self.gen == self.slot.gen.get()
    }

    #[inline(always)]
    pub fn get_unchecked(self) -> *mut T {
        unsafe {
            self.slot
                .data
                .get()
                .cast::<u8>()
                .add(self.offset)
                .cast::<T>()
        }
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
            _ty: PhantomData,
            gen: self.gen,
            slot: self.slot,
            offset: data as usize - self.slot.data.get() as usize,
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
