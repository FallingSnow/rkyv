//! [`Archive`] implementations for core types.

use crate::{
    offset_of, AllocDeserializer, Archive, ArchiveCopy, ArchiveRef, Archived, RelPtr,
    Serialize, Serializer, SerializeRef, Deserialize, DeserializeRef,
};
use core::{
    alloc,
    borrow::Borrow,
    cmp, fmt,
    hash::{Hash, Hasher},
    marker::PhantomData,
    num::{
        NonZeroI128, NonZeroI16, NonZeroI32, NonZeroI64, NonZeroI8, NonZeroU128, NonZeroU16,
        NonZeroU32, NonZeroU64, NonZeroU8,
    },
    ops::{Deref, DerefMut},
    ptr, slice, str,
    sync::atomic::{
        self, AtomicBool, AtomicI16, AtomicI32, AtomicI64, AtomicI8, AtomicU16, AtomicU32,
        AtomicU64, AtomicU8,
    },
};
pub mod range;
#[cfg(feature = "validation")]
pub mod validation;

/// A strongly typed relative reference.
///
/// This is the reference type for all sized archived types. It uses [`RelPtr`]
/// under the hood.
#[repr(transparent)]
#[derive(Debug)]
pub struct ArchivedRef<T> {
    ptr: RelPtr,
    _phantom: PhantomData<T>,
}

impl<T> ArchivedRef<T> {
    unsafe fn new(from: usize, to: usize) -> Self {
        Self {
            ptr: RelPtr::new(from, to),
            _phantom: PhantomData,
        }
    }
}

impl<T> Deref for ArchivedRef<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.ptr.as_ptr() }
    }
}

impl<T> DerefMut for ArchivedRef<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.ptr.as_mut_ptr() }
    }
}

impl<T: Hash> Hash for ArchivedRef<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.deref().hash(state)
    }
}

impl<T: PartialEq> PartialEq for ArchivedRef<T> {
    fn eq(&self, other: &Self) -> bool {
        self.deref().eq(other.deref())
    }
}

impl<T: Eq> Eq for ArchivedRef<T> {}

impl<T: Archive> ArchiveRef for T {
    type Archived = T::Archived;
    type Reference = ArchivedRef<Self::Archived>;

    fn resolve_ref(&self, from: usize, to: usize) -> Self::Reference {
        unsafe { ArchivedRef::new(from, to) }
    }
}

impl<T: Serialize<S>, S: Serializer + ?Sized> SerializeRef<S> for T {
    fn serialize_ref(&self, serializer: &mut S) -> Result<usize, S::Error> {
        Ok(serializer.serialize(self)?)
    }
}

impl<T: Archive, D: AllocDeserializer + ?Sized> DeserializeRef<T, D> for <T as ArchiveRef>::Reference
where
    T::Archived: Deserialize<T, D>,
{
    unsafe fn deserialize_ref(&self, deserializer: &mut D) -> *mut T {
        let ptr = deserializer.alloc(alloc::Layout::new::<T>()).cast::<T>();
        let deserialized = self.deserialize(deserializer);
        ptr.write(deserialized);
        ptr
    }
}

/// A strongly typed relative slice reference.
///
/// This is the reference type for all archived slices. It uses [`RelPtr`] under
/// the hood and stores an additional length parameter.
#[cfg_attr(feature = "strict", repr(C))]
#[derive(Debug)]
pub struct ArchivedSlice<T> {
    ptr: RelPtr,
    len: u32,
    _phantom: PhantomData<T>,
}

impl<T> ArchivedSlice<T> {
    pub unsafe fn new(from: usize, to: usize, len: usize) -> Self {
        Self {
            ptr: RelPtr::new(from + offset_of!(Self, ptr), to),
            len: len as u32,
            _phantom: PhantomData,
        }
    }
}

impl<T> Deref for ArchivedSlice<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        unsafe { slice::from_raw_parts(self.ptr.as_ptr(), self.len as usize) }
    }
}

impl<T> DerefMut for ArchivedSlice<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { slice::from_raw_parts_mut(self.ptr.as_mut_ptr(), self.len as usize) }
    }
}

impl<T: Eq> Eq for ArchivedSlice<T> {}

impl<T: Hash> Hash for ArchivedSlice<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.deref().hash(state)
    }
}

impl<T: Ord> Ord for ArchivedSlice<T> {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        Ord::cmp(&**self, &**other)
    }
}

impl<T: PartialEq> PartialEq for ArchivedSlice<T> {
    fn eq(&self, other: &Self) -> bool {
        self.deref().eq(other.deref())
    }
}

impl<T: PartialOrd> PartialOrd for ArchivedSlice<T> {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        PartialOrd::partial_cmp(&**self, &**other)
    }
}

#[cfg(not(feature = "std"))]
impl<T: ArchiveCopy> ArchiveRef for [T] {
    type Archived = [T::Archived];
    type Reference = ArchivedSlice<T::Archived>;

    fn archive_ref<W: Write + ?Sized>(&self, writer: &mut W) -> Result<usize, W::Error> {
        if !self.is_empty() {
            let result = writer.align_for::<T>()?;
            let bytes = unsafe {
                slice::from_raw_parts(
                    self.as_ptr().cast::<u8>(),
                    core::mem::size_of::<T>() * self.len(),
                )
            };
            writer.write(bytes)?;
            Ok(result)
        } else {
            Ok(0)
        }
    }
}

#[cfg(not(feature = "std"))]
impl<T: ArchiveCopy, D: AllocDeserializer + ?Sized> DeserializeRef<[T], D> for <[T] as ArchiveRef>::Reference
where
    T::Archived: Deserialize<T, D>,
{
    unsafe fn deserialize_ref(&self, deserializer: &mut D) -> *mut [T] {
        let result = deserializer.alloc(alloc::Layout::array::<T>(self.len()).unwrap()).cast::<T>();
        ptr::copy_nonoverlapping(self.as_ptr(), result, self.len());
        slice::from_raw_parts_mut(result, self.len())
    }
}

impl<T: ?Sized> Archive for PhantomData<T> {
    type Archived = PhantomData<T>;
    type Resolver = ();

    fn resolve(&self, _: usize, _: Self::Resolver) -> Self::Archived {
        PhantomData
    }
}

impl<T: ?Sized, S: Serializer + ?Sized> Serialize<S> for PhantomData<T> {
    fn serialize(&self, _: &mut S) -> Result<Self::Resolver, S::Error> {
        Ok(())
    }
}

unsafe impl<T: ?Sized> ArchiveCopy for PhantomData<T> {}

impl<T: ?Sized, D: ?Sized> Deserialize<PhantomData<T>, D> for PhantomData<T> {
    fn deserialize(&self, _: &mut D) -> PhantomData<T> {
        PhantomData
    }
}

macro_rules! impl_primitive {
    ($type:ty) => {
        impl Archive for $type
        where
            $type: Copy,
        {
            type Archived = Self;
            type Resolver = ();

            fn resolve(&self, _: usize, _: Self::Resolver) -> Self::Archived {
                *self
            }
        }

        impl<S: Serializer + ?Sized> Serialize<S> for $type {
            fn serialize(&self, _: &mut S) -> Result<Self::Resolver, S::Error> {
                Ok(())
            }
        }

        unsafe impl ArchiveCopy for $type {}

        impl<D: ?Sized> Deserialize<$type, D> for $type
        where
            $type: Copy,
        {
            fn deserialize(&self, _: &mut D) -> $type {
                *self
            }
        }
    };
}

impl_primitive!(());
impl_primitive!(bool);
impl_primitive!(i8);
impl_primitive!(i16);
impl_primitive!(i32);
impl_primitive!(i64);
impl_primitive!(i128);
impl_primitive!(u8);
impl_primitive!(u16);
impl_primitive!(u32);
impl_primitive!(u64);
impl_primitive!(u128);
impl_primitive!(f32);
impl_primitive!(f64);
impl_primitive!(char);
impl_primitive!(NonZeroI8);
impl_primitive!(NonZeroI16);
impl_primitive!(NonZeroI32);
impl_primitive!(NonZeroI64);
impl_primitive!(NonZeroI128);
impl_primitive!(NonZeroU8);
impl_primitive!(NonZeroU16);
impl_primitive!(NonZeroU32);
impl_primitive!(NonZeroU64);
impl_primitive!(NonZeroU128);

/// The resolver for atomic types.
pub struct AtomicResolver;

macro_rules! impl_atomic {
    ($type:ty) => {
        impl Archive for $type {
            type Archived = Self;
            type Resolver = AtomicResolver;

            fn resolve(&self, _pos: usize, _resolver: AtomicResolver) -> $type {
                <$type>::new(self.load(atomic::Ordering::Relaxed))
            }
        }

        impl<S: Serializer + ?Sized> Serialize<S> for $type {
            fn serialize(
                &self,
                _: &mut S,
            ) -> Result<Self::Resolver, S::Error> {
                Ok(AtomicResolver)
            }
        }

        impl<D: ?Sized> Deserialize<$type, D> for $type {
            fn deserialize(&self, _: &mut D) -> $type {
                <$type>::new(self.load(atomic::Ordering::Relaxed))
            }
        }
    };
}

impl_atomic!(AtomicBool);
impl_atomic!(AtomicI8);
impl_atomic!(AtomicI16);
impl_atomic!(AtomicI32);
impl_atomic!(AtomicI64);
impl_atomic!(AtomicU8);
impl_atomic!(AtomicU16);
impl_atomic!(AtomicU32);
impl_atomic!(AtomicU64);

#[cfg(not(feature = "strict"))]
macro_rules! peel_tuple {
    ($type:ident $index:tt, $($type_rest:ident $index_rest:tt,)*) => { impl_tuple! { $($type_rest $index_rest,)* } };
}

#[cfg(not(feature = "strict"))]
macro_rules! impl_tuple {
    () => ();
    ($($type:ident $index:tt,)+) => {
        unsafe impl<$($type: ArchiveCopy),+> ArchiveCopy for ($($type,)+) {}

        impl<$($type: Archive),+> Archive for ($($type,)+) {
            type Archived = ($($type::Archived,)+);
            type Resolver = ($($type::Resolver,)+);

            fn resolve(&self, pos: usize, resolver: Self::Resolver) -> Self::Archived {
                #[allow(clippy::unneeded_wildcard_pattern)]
                let rev = ($(self.$index.resolve(pos + memoffset::offset_of_tuple!(Self::Archived, $index), resolver.$index),)+);
                ($(rev.$index,)+)
            }
        }

        impl<$($type: Serialize<S>),+, S: Serializer + ?Sized> Serialize<S> for ($($type,)+) {
            fn serialize(&self, serializer: &mut S) -> Result<Self::Resolver, S::Error> {
                let rev = ($(self.$index.serialize(serializer)?,)+);
                Ok(($(rev.$index,)+))
            }
        }

        impl<D: ?Sized, $($type: Archive),+> Deserialize<($($type,)+), D> for ($($type::Archived,)+)
        where
            $($type::Archived: Deserialize<$type, D>,)+
        {
            fn deserialize(&self, deserializer: &mut D) -> ($($type,)+) {
                let rev = ($(&self.$index,)+);
                ($(rev.$index.deserialize(deserializer),)+)
            }
        }

        peel_tuple! { $($type $index,)+ }
    };
}

#[cfg(not(feature = "strict"))]
impl_tuple! { T11 11, T10 10, T9 9, T8 8, T7 7, T6 6, T5 5, T4 4, T3 3, T2 2, T1 1, T0 0, }

#[cfg(not(feature = "const_generics"))]
macro_rules! impl_array {
    () => ();
    ($len:literal, $($rest:literal,)*) => {
        unsafe impl<T: ArchiveCopy> ArchiveCopy for [T; $len] {}

        impl<T: Archive> Archive for [T; $len] {
            type Archived = [T::Archived; $len];
            type Resolver = [T::Resolver; $len];

            fn resolve(&self, pos: usize, resolver: Self::Resolver) -> Self::Archived {
                let mut resolvers = core::mem::MaybeUninit::new(resolver);
                let resolvers_ptr = resolvers.as_mut_ptr().cast::<T::Resolver>();
                let mut result = core::mem::MaybeUninit::<Self::Archived>::uninit();
                let result_ptr = result.as_mut_ptr().cast::<T::Archived>();
                #[allow(clippy::reversed_empty_ranges)]
                for i in 0..$len {
                    unsafe {
                        result_ptr.add(i).write(self[i].resolve(pos + i * core::mem::size_of::<T>(), resolvers_ptr.add(i).read()));
                    }
                }
                unsafe {
                    result.assume_init()
                }
            }
        }

        impl<T: Serialize<S>, S: Serializer + ?Sized> Serialize<S> for [T; $len] {
            fn serialize(&self, serializer: &mut S) -> Result<Self::Resolver, S::Error> {
                let mut result = core::mem::MaybeUninit::<Self::Resolver>::uninit();
                let result_ptr = result.as_mut_ptr().cast::<T::Resolver>();
                #[allow(clippy::reversed_empty_ranges)]
                for i in 0..$len {
                    unsafe {
                        result_ptr.add(i).write(self[i].serialize(serializer)?);
                    }
                }
                unsafe { Ok(result.assume_init()) }
            }
        }

        impl<T: Archive, D: ?Sized> Deserialize<[T; $len], D> for [T::Archived; $len]
        where
            T::Archived: Deserialize<T, D>,
        {
            fn deserialize(&self, deserializer: &mut D) -> [T; $len] {
                let mut result = core::mem::MaybeUninit::<[T; $len]>::uninit();
                let result_ptr = result.as_mut_ptr().cast::<T>();
                #[allow(clippy::reversed_empty_ranges)]
                for i in 0..$len {
                    unsafe {
                        result_ptr.add(i).write(self[i].deserialize(deserializer));
                    }
                }
                unsafe { result.assume_init() }
            }
        }

        impl_array! { $($rest,)* }
    };
}

#[cfg(not(feature = "const_generics"))]
impl_array! { 31, 30, 29, 28, 27, 26, 25, 24, 23, 22, 21, 20, 19, 18, 17, 16, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0, }

#[cfg(feature = "const_generics")]
unsafe impl<T: ArchiveCopy, const N: usize> ArchiveCopy for [T; N] {}

#[cfg(feature = "const_generics")]
impl<T: Archive, const N: usize> Archive for [T; N] {
    type Archived = [T::Archived; N];
    type Resolver = [T::Resolver; N];

    fn resolve(&self, pos: usize, resolver: Self::Resolver) -> Self::Archived {
        let mut resolvers = core::mem::MaybeUninit::new(resolver);
        let resolvers_ptr = resolvers.as_mut_ptr().cast::<T>();
        let mut result = core::mem::MaybeUninit::<Self::Archived>::uninit();
        let result_ptr = result.as_mut_ptr().cast::<T::Archived>();
        for i in 0..N {
            unsafe {
                result_ptr.add(i).write(
                    self[i].resolve(pos + i * core::mem::size_of::<T>(), resolvers_ptr.add(i).read()),
                );
            }
        }
        unsafe { result.assume_init() }
    }
}

#[cfg(feature = "const_generics")]
impl<T: Serialize<W>, W: Write + ?Sized, const N: usize> Serialize<W> for [T; N] {
    fn serialize(&self, writer: &mut W) -> Result<Self::Resolver, W::Error> {
        let mut result = core::mem::MaybeUninit::<Self::Resolver>::uninit();
        let result_ptr = result.as_mut_ptr().cast::<T::Resolver>();
        for i in 0..N {
            unsafe {
                result_ptr.add(i).write(self[i].serialize(writer)?);
            }
        }
        unsafe { Ok(result.assume_init()) }
    }
}

#[cfg(feature = "const_generics")]
impl<T: Archive, const N: usize> Deserialize<[T; N]> for [T::Archived; N]
where
    T::Archived: Deserialize<T>,
{
    fn deserialize(&self) -> [T; N] {
        let mut result = core::mem::MaybeUninit::<[T; N]>::uninit();
        let result_ptr = result.as_mut_ptr().cast::<T>();
        for i in 0..N {
            unsafe {
                result_ptr.add(i).write(self[i].deserialize());
            }
        }
        unsafe { result.assume_init() }
    }
}

/// A reference to an archived string slice.
///
/// It implements a handful of helper functions and traits to function similarly
/// to [`prim@str`].
#[repr(transparent)]
#[derive(Debug)]
pub struct ArchivedStringSlice {
    slice: ArchivedSlice<u8>,
}

impl ArchiveRef for str {
    type Archived = str;
    type Reference = ArchivedStringSlice;

    fn resolve_ref(&self, from: usize, to: usize) -> Self::Reference {
        Self::Reference {
            slice: unsafe { ArchivedSlice::new(from, to, self.len()) },
        }
    }
}

impl<S: Serializer + ?Sized> SerializeRef<S> for str {
    fn serialize_ref(&self, serializer: &mut S) -> Result<usize, S::Error> {
        let result = serializer.pos();
        serializer.write(self.as_bytes())?;
        Ok(result)
    }
}

impl<D: AllocDeserializer + ?Sized> DeserializeRef<str, D> for <str as ArchiveRef>::Reference {
    unsafe fn deserialize_ref(&self, deserializer: &mut D) -> *mut str {
        let bytes = deserializer.alloc(alloc::Layout::array::<u8>(self.len()).unwrap());
        ptr::copy_nonoverlapping(self.as_ptr(), bytes, self.len());
        let slice = slice::from_raw_parts_mut(bytes, self.len());
        str::from_utf8_unchecked_mut(slice)
    }
}

impl Deref for ArchivedStringSlice {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        unsafe { str::from_utf8_unchecked(&*self.slice) }
    }
}

impl DerefMut for ArchivedStringSlice {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { str::from_utf8_unchecked_mut(&mut *self.slice) }
    }
}

impl Borrow<str> for ArchivedStringSlice {
    fn borrow(&self) -> &str {
        &**self
    }
}

impl Eq for ArchivedStringSlice {}

impl Hash for ArchivedStringSlice {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (**self).hash(state)
    }
}

impl Ord for ArchivedStringSlice {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        Ord::cmp(&**self, &**other)
    }
}

impl PartialEq for ArchivedStringSlice {
    fn eq(&self, other: &Self) -> bool {
        PartialEq::eq(&**self, &**other)
    }
}

impl PartialOrd for ArchivedStringSlice {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        PartialOrd::partial_cmp(&**self, &**other)
    }
}

impl fmt::Display for ArchivedStringSlice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&**self, f)
    }
}

/// An archived [`Option`].
///
/// It functions identically to [`Option`] but has a different internal
/// representation to allow for archiving.
#[derive(Debug)]
#[repr(u8)]
pub enum ArchivedOption<T> {
    /// No value
    None,
    /// Some value `T`
    Some(T),
}

impl<T> ArchivedOption<T> {
    /// Returns `true` if the option is a `None` value.
    pub fn is_none(&self) -> bool {
        match self {
            ArchivedOption::None => true,
            ArchivedOption::Some(_) => false,
        }
    }

    /// Returns `true` if the option is a `Some` value.
    pub fn is_some(&self) -> bool {
        match self {
            ArchivedOption::None => false,
            ArchivedOption::Some(_) => true,
        }
    }

    /// Converts to an `Option<&T>`.
    pub fn as_ref(&self) -> Option<&T> {
        match self {
            ArchivedOption::None => None,
            ArchivedOption::Some(value) => Some(value),
        }
    }

    /// Converts to an `Option<&mut T>`.
    pub fn as_mut(&mut self) -> Option<&mut T> {
        match self {
            ArchivedOption::None => None,
            ArchivedOption::Some(value) => Some(value),
        }
    }

    /// Inserts `v` into the option if it is `None`, then returns a mutable
    /// reference to the contained value.
    pub fn get_or_insert(&mut self, v: T) -> &mut T {
        self.get_or_insert_with(move || v)
    }

    /// Inserts a value computed from `f` into the option if it is `None`, then
    /// returns a mutable reference to the contained value.
    pub fn get_or_insert_with<F: FnOnce() -> T>(&mut self, f: F) -> &mut T {
        if let ArchivedOption::Some(ref mut value) = self {
            value
        } else {
            *self = ArchivedOption::Some(f());
            self.as_mut().unwrap()
        }
    }
}

#[allow(dead_code)]
#[repr(u8)]
enum ArchivedOptionTag {
    None,
    Some,
}

#[repr(C)]
struct ArchivedOptionVariantSome<T>(ArchivedOptionTag, T);

impl<T: Archive> Archive for Option<T> {
    type Archived = ArchivedOption<T::Archived>;
    type Resolver = Option<T::Resolver>;

    fn resolve(&self, pos: usize, resolver: Option<T::Resolver>) -> Self::Archived {
        match resolver {
            None => ArchivedOption::None,
            Some(resolver) => ArchivedOption::Some(self.as_ref().unwrap().resolve(
                pos + offset_of!(ArchivedOptionVariantSome<T::Archived>, 1),
                resolver
            )),
        }
    }
}

impl<T: Serialize<S>, S: Serializer + ?Sized> Serialize<S> for Option<T> {
    fn serialize(&self, serializer: &mut S) -> Result<Self::Resolver, S::Error> {
        self.as_ref().map(|value| value.serialize(serializer)).transpose()
    }
}

impl<T: Archive, D: ?Sized> Deserialize<Option<T>, D> for Archived<Option<T>>
where
    T::Archived: Deserialize<T, D>,
{
    fn deserialize(&self, deserializer: &mut D) -> Option<T> {
        self.as_ref().map(|value| value.deserialize(deserializer))
    }
}

impl<T: Eq> Eq for ArchivedOption<T> {}

impl<T: Hash> Hash for ArchivedOption<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_ref().hash(state)
    }
}

impl<T: Ord> Ord for ArchivedOption<T> {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        self.as_ref().cmp(&other.as_ref())
    }
}

impl<T: PartialEq> PartialEq for ArchivedOption<T> {
    fn eq(&self, other: &Self) -> bool {
        self.as_ref().eq(&other.as_ref())
    }
}

impl<T: PartialOrd> PartialOrd for ArchivedOption<T> {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        self.as_ref().partial_cmp(&other.as_ref())
    }
}

impl<T, U: PartialEq<T>> PartialEq<Option<T>> for ArchivedOption<U> {
    fn eq(&self, other: &Option<T>) -> bool {
        if let ArchivedOption::Some(self_value) = self {
            if let Some(other_value) = other {
                self_value.eq(other_value)
            } else {
                false
            }
        } else {
            other.is_none()
        }
    }
}

impl<T: PartialEq<U>, U> PartialEq<ArchivedOption<T>> for Option<U> {
    fn eq(&self, other: &ArchivedOption<T>) -> bool {
        other.eq(self)
    }
}
