use std::{
    cell::Cell,
    mem::{self, MaybeUninit},
    num::NonZeroU64,
    ptr::{addr_of_mut, NonNull},
};

// === Gen Allocation === //

pub fn alloc_gen() -> NonZeroU64 {
    thread_local! {
        static GEN: Cell<NonZeroU64> = const {
            Cell::new(match NonZeroU64::new(GEN_ENTITY_MIN) {
                Some(val) => val,
                None => unreachable!(),
            })
        };
    }

    GEN.with(|v| {
        let id = v.get();
        v.set(id.checked_add(1).expect("too many IDs"));
        id
    })
}

// === AllocationSlot === //

const DANGLING_ERR: &str = "attempted to deref a dead pointer";

#[repr(C)]
struct AllocationSlotVirtual<T: ?Sized> {
    gen: u64,
    value: T,
}

// === Ptr === //

const GEN_NONE: u64 = 0;
const GEN_ALLOCATED: u64 = 1;
const GEN_NEVER: u64 = 2;
const GEN_ENTITY_MIN: u64 = 3;

pub struct Ptr<T: ?Sized> {
    // Invariants: This must always point to a readable `u64`. If `gen` is non-zero, this value is
    // writable. If `gen` is neither zero nor one, this points to a readable and writable
    // `AllocationSlotVirtual<T>`.
    data: NonNull<AllocationSlotVirtual<T>>,
}

impl<T: ?Sized> Ptr<T> {
    // === Allocation === //

    #[inline(always)]
    pub fn alloc() -> Self
    where
        T: Sized,
    {
        #[repr(C)]
        struct AllocationSlot<T> {
            _gen: u64,
            _value: MaybeUninit<T>,
        }

        Self {
            data: NonNull::from(Box::leak(Box::new(AllocationSlot::<T> {
                _gen: GEN_ALLOCATED,
                _value: MaybeUninit::uninit(),
            })))
            .cast(),
        }
    }

    #[inline(always)]
    pub fn dealloc(self) {
        todo!()
    }

    // === Generation queries === //

    #[inline(always)]
    fn gen_ptr(self) -> NonNull<u64> {
        self.data.cast::<u64>()
    }

    #[inline(always)]
    pub fn gen(self) -> u64 {
        unsafe { *self.gen_ptr().as_ptr() }
    }

    #[inline(always)]
    unsafe fn set_gen(self, gen: u64) {
        unsafe { *self.gen_ptr().as_ptr() = gen };
    }

    #[inline(always)]
    pub fn get_unchecked(self) -> NonNull<T> {
        unsafe { NonNull::new(addr_of_mut!((*self.data.as_ptr()).value)).unwrap_unchecked() }
    }

    #[inline(always)]
    pub fn is_allocated(self) -> bool {
        self.gen() != GEN_NONE
    }

    #[inline(always)]
    pub fn is_allocated_and_init(self) -> bool {
        self.gen() >= GEN_ENTITY_MIN
    }

    #[inline(always)]
    pub fn try_get(self) -> Option<NonNull<T>> {
        if self.is_allocated_and_init() {
            Some(self.get_unchecked())
        } else {
            None
        }
    }

    #[inline(always)]
    pub fn get(self) -> NonNull<T> {
        self.try_get().expect(DANGLING_ERR)
    }

    // === Pointer management === //

    #[inline(always)]
    pub fn write_new(self, value: T) -> Option<T>
    where
        T: Sized,
    {
        self.write(alloc_gen(), value)
    }

    #[inline(always)]
    pub fn write(self, gen: NonZeroU64, value: T) -> Option<T>
    where
        T: Sized,
    {
        debug_assert_ne!(gen.get(), GEN_ALLOCATED);

        // Ensure that this ptr has a backing allocation and write the value to it
        let value = match self.gen() {
            0 => panic!("attempted to initialize un-allocated Ptr"),
            1 => unsafe {
                self.get_unchecked().as_ptr().write(value);
                None
            },
            _ => Some(unsafe { mem::replace(self.get_unchecked().as_mut(), value) }),
        };

        // Update the generation
        unsafe { self.set_gen(gen.get()) };

        value
    }

    #[inline(always)]
    pub fn take(self) -> Option<T>
    where
        T: Sized,
    {
        if self.is_allocated_and_init() {
            // Mark this slot as allocated but empty.
            unsafe { self.set_gen(GEN_ALLOCATED) };

            // Take the value from the slot, leaving it uninitialized.
            let value = unsafe { self.get_unchecked().as_ptr().read() };
            Some(value)
        } else {
            None
        }
    }

    #[inline(always)]
    #[must_use]
    pub fn try_destroy(self) -> bool {
        if self.is_allocated_and_init() {
            // Mark this slot as allocated but empty. This happens first because `drop_in_place`
            // calls out to user code.
            unsafe { self.set_gen(GEN_ALLOCATED) };

            // Drop the value in the slot without moving it, leaving it uninitialized.
            unsafe { self.get_unchecked().as_ptr().drop_in_place() };

            true
        } else {
            false
        }
    }

    #[inline(always)]
    pub fn destroy(self) {
        assert!(self.try_destroy(), "{DANGLING_ERR}");
    }

    #[inline(always)]
    pub fn as_wide_ref_prim(self) -> WideRef<T> {
        WideRef {
            gen: NonZeroU64::new(self.gen()).unwrap_or(NonZeroU64::new(GEN_NEVER).unwrap()),
            slot: self.gen_ptr(),
            data: self.get_unchecked(),
        }
    }

    #[inline(always)]
    pub fn as_wide_ref(self) -> T::WideWrapper
    where
        T: Struct,
    {
        WideWrapper::from_raw(self.as_wide_ref_prim())
    }
}

impl<T: ?Sized> Copy for Ptr<T> {}

impl<T: ?Sized> Clone for Ptr<T> {
    fn clone(&self) -> Self {
        *self
    }
}

// === WideRef === //

pub struct WideRef<T: ?Sized> {
    gen: NonZeroU64,

    // Invariants: This pointer is always readable. If `*slot == gen`, `data` is valid.
    slot: NonNull<u64>,

    // Invariants: This pointer has read and write access to its target if valid.
    data: NonNull<T>,
}

impl<T: ?Sized> WideRef<T> {
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
        self.try_get().expect(DANGLING_ERR)
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
        self.try_read().expect(DANGLING_ERR)
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
        self.try_write(value).expect(DANGLING_ERR)
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
        let ptr = unsafe {
            // Safety: this is a valid pointer to some data.
            $crate::subfield_internals::addr_of_mut!((*ptr).$field)
        };

        unsafe {
            // Safety: this field will not expire until the parent structure has expired.
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
    type WideWrapper: WideWrapper<Pointee = Self>;
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
