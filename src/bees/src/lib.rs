use std::{
    cell::{Cell, UnsafeCell},
    mem::MaybeUninit,
    num::NonZeroU64,
    ptr::NonNull,
};

// === Util === //

mod util {
    use std::{hash, marker::PhantomData};

    pub struct ConstSafeBuildHasherDefault<T>(PhantomData<fn(T) -> T>);

    impl<T> ConstSafeBuildHasherDefault<T> {
        pub const fn new() -> Self {
            Self(PhantomData)
        }
    }

    impl<T> Default for ConstSafeBuildHasherDefault<T> {
        fn default() -> Self {
            Self::new()
        }
    }

    impl<T: hash::Hasher + Default> hash::BuildHasher for ConstSafeBuildHasherDefault<T> {
        type Hasher = T;

        fn build_hasher(&self) -> Self::Hasher {
            T::default()
        }
    }

    #[derive(Default)]
    pub struct NoOpHasher(u64);

    impl hash::Hasher for NoOpHasher {
        fn write_u64(&mut self, i: u64) {
            debug_assert_eq!(self.0, 0);
            self.0 = i;
        }

        fn write(&mut self, _bytes: &[u8]) {
            unimplemented!("This is only supported for `u64`s.")
        }

        fn finish(&self) -> u64 {
            self.0
        }
    }

    pub type NopHashBuilder = ConstSafeBuildHasherDefault<NoOpHasher>;
    pub type NopHashMap<K, V> = hashbrown::HashMap<K, V, NopHashBuilder>;
    // pub type NopHashSet<T> = hashbrown::HashSet<T, NopHashBuilder>;
}

use util::*;

// === Database === //

mod db {
    use std::{
        cell::RefCell,
        num::NonZeroU64,
        ptr::NonNull,
        sync::atomic::{AtomicU64, Ordering::Relaxed},
    };

    use super::*;

    pub(crate) fn use_object_db<R>(
        f: impl FnOnce(&mut NopHashMap<NonZeroU64, *mut u64>) -> R,
    ) -> R {
        thread_local! {
            static OBJECT_DB: RefCell<NopHashMap<NonZeroU64, *mut u64>> =
                const { RefCell::new(NopHashMap::with_hasher(ConstSafeBuildHasherDefault::new())) };
        }

        OBJECT_DB.with(|v| f(&mut v.borrow_mut()))
    }

    pub(crate) fn gen() -> NonZeroU64 {
        static GEN: AtomicU64 = AtomicU64::new(1);
        NonZeroU64::new(GEN.fetch_add(1, Relaxed)).unwrap()
    }

    pub(crate) fn alloc<T: 'static>(len: usize) -> NonNull<[Generational<T>]> {
        NonNull::from(Box::leak(Box::from_iter(
            (0..len).map(|_| Generational::new_empty()),
        )))
    }

    pub(crate) unsafe fn realloc<T: 'static>(
        _alloc: NonNull<[Generational<T>]>,
        size: usize,
    ) -> NonNull<[Generational<T>]> {
        todo!();
    }

    pub(crate) unsafe fn dealloc<T: 'static>(_alloc: NonNull<[Generational<T>]>) {
        // TODO
    }
}

// === Arena === //

#[derive_where(Debug, Copy, Clone)]
pub struct Allocation<T: 'static> {
    values: NonNull<[Generational<T>]>,
}

impl<T> Allocation<T> {
    pub fn new(len: usize) -> Self {
        Self {
            values: db::alloc(len),
        }
    }

    fn values(self) -> &'static [Generational<T>] {
        unsafe { &self.values.as_ref() }
    }

    pub fn put_with_gen(self, index: usize, gen: NonZeroU64, value: T) -> Ref<T> {
        let slot = &self.values()[index];

        unsafe { slot.replace(Some((gen, value))) };

        Ref {
            gen,
            gen_ptr: slot.gen.as_ptr(),
            value: slot.value_ptr(),
        }
    }

    pub fn put(self, index: usize, value: T) -> Ref<T> {
        self.put_with_gen(index, db::gen(), value)
    }

    pub fn take(self, index: usize) -> Option<T> {
        unsafe { self.values()[index].replace(None) }
    }

    pub fn try_get(self, index: usize) -> Option<Ref<T>> {
        let slot = &self.values()[index];

        if slot.is_full() {
            Some(Ref {
                gen: NonZeroU64::new(slot.gen()).unwrap(),
                gen_ptr: slot.gen_ptr(),
                value: slot.value_ptr(),
            })
        } else {
            None
        }
    }

    pub fn get(self, index: usize) -> Ref<T> {
        self.try_get(index).unwrap()
    }

    pub fn len(self) -> usize {
        self.values.len()
    }

    pub fn dealloc(self) {
        // Disconnect references
        for slot in self.values() {
            unsafe { slot.replace(None) };
        }

        unsafe { db::dealloc(self.values) }
    }
}

struct Generational<T> {
    gen: Cell<u64>,
    value: UnsafeCell<MaybeUninit<T>>,
}

impl<T> Generational<T> {
    pub const fn new_empty() -> Self {
        Self {
            gen: Cell::new(0),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    pub fn is_full(&self) -> bool {
        self.gen.get() != 0
    }

    pub fn gen_ptr(&self) -> *mut u64 {
        self.gen.as_ptr()
    }

    pub fn gen(&self) -> u64 {
        self.gen.get()
    }

    pub fn value_ptr(&self) -> *mut T {
        self.value.get() as *mut T
    }

    pub unsafe fn replace(&self, value: Option<(NonZeroU64, T)>) -> Option<T> {
        let old = if self.is_full() {
            db::use_object_db(|db| db.remove(&NonZeroU64::new(self.gen()).unwrap()));

            Some(unsafe { self.value_ptr().read() })
        } else {
            None
        };

        if let Some((gen, value)) = value {
            // Replace entry in Object DB
            db::use_object_db(|db| match db.entry(gen) {
                hashbrown::hash_map::Entry::Occupied(_) => panic!("Reused generation {gen:?}"),
                hashbrown::hash_map::Entry::Vacant(entry) => {
                    entry.insert(self.gen_ptr());
                }
            });

            self.gen.set(gen.get());
            self.value_ptr().write(value);
        } else {
            self.gen.set(0);
        }

        old
    }
}

impl<T> Drop for Generational<T> {
    fn drop(&mut self) {
        if self.is_full() {
            unsafe { self.value_ptr().drop_in_place() };
        }
    }
}

// === Ref === //

const DANGLING_ERR: &str = "attempted to deref a dead pointer";

#[derive_where(Copy, Clone)]
pub struct Ref<T: 'static> {
    gen_ptr: *mut u64,
    gen: NonZeroU64,
    value: *mut T,
}

impl<T> Ref<T> {
    #[inline(always)]
    pub fn is_alive(self) -> bool {
        self.gen.get() == unsafe { *self.gen_ptr }
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

use derive_where::derive_where;
pub(crate) use func_disambiguator_sealed::FuncDisambiguator;

// === MovableRef === //

#[derive_where(Clone)]
pub struct MovableRef<T> {
    gen_ptr: *mut u64,
    gen: NonZeroU64,
    value: Cell<*mut T>,
}

impl<T> MovableRef<T> {
    pub fn force_resolve_prim(&self) -> Ref<T> {
        Ref {
            gen_ptr: self.gen_ptr,
            gen: self.gen,
            value: self.value.get(),
        }
    }

    pub fn force_resolve(&self) -> T::Wrapper
    where
        T: Struct,
    {
        self.force_resolve_prim().wrap()
    }

    pub fn repair_resolve_prim(&self) -> Ref<T> {
        let resolved = self.force_resolve_prim();
        if resolved.is_alive() {
            return resolved;
        }

        todo!();
    }

    pub fn repair_resolve(&self) -> T::Wrapper
    where
        T: Struct,
    {
        self.repair_resolve_prim().wrap()
    }
}

// === ThinRef === //

// TODO: Implement `ThinRef`

// === Struct === //

pub trait Struct: 'static {
    type Wrapper: RefWrapper<Pointee = Self>;
}

pub trait RefWrapper: Copy {
    type Pointee: 'static;

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
