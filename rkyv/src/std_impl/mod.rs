//! [`Archive`] implementations for std types.

pub mod chd;
pub mod shared;
#[cfg(feature = "validation")]
pub mod validation;

use crate::{
    core_impl::ArchivedSlice, AllocDeserializer, Archive, ArchiveRef, Archived, Reference,
    Serialize, SerializeRef, Deserialize, DeserializeRef, Serializer,
};
use core::{
    borrow::Borrow,
    fmt,
    ops::{Deref, DerefMut, Index, IndexMut},
    pin::Pin,
    slice,
};
use std::alloc;

pub struct GlobalAllocDeserializer;

impl AllocDeserializer for GlobalAllocDeserializer {
    unsafe fn alloc(&mut self, layout: alloc::Layout) -> *mut u8 {
        std::alloc::alloc(layout)
    }
}

/// An archived [`String`].
///
/// Uses [`ArchivedStringSlice`](crate::core_impl::ArchivedStringSlice) under
/// the hood.
#[derive(Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct ArchivedString(Reference<str>);

impl ArchivedString {
    /// Extracts a string slice containing the entire `ArchivedString`.
    pub fn as_str(&self) -> &str {
        &**self
    }

    /// Converts an `ArchivedString` into a mutable string slice.
    pub fn as_mut_str(&mut self) -> &mut str {
        &mut **self
    }

    /// Gets the value of this archived string as a pinned mutable reference.
    pub fn str_pin(self: Pin<&mut Self>) -> Pin<&mut str> {
        unsafe { self.map_unchecked_mut(|s| &mut **s) }
    }
}

impl Deref for ArchivedString {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl DerefMut for ArchivedString {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

impl Borrow<str> for ArchivedString {
    fn borrow(&self) -> &str {
        self.0.borrow()
    }
}

impl PartialEq<&str> for ArchivedString {
    fn eq(&self, other: &&str) -> bool {
        PartialEq::eq(&**self, *other)
    }
}

impl PartialEq<ArchivedString> for &str {
    fn eq(&self, other: &ArchivedString) -> bool {
        PartialEq::eq(&**other, *self)
    }
}

impl PartialEq<String> for ArchivedString {
    fn eq(&self, other: &String) -> bool {
        PartialEq::eq(&**self, &**other)
    }
}

impl PartialEq<ArchivedString> for String {
    fn eq(&self, other: &ArchivedString) -> bool {
        PartialEq::eq(&**other, &**self)
    }
}

impl fmt::Display for ArchivedString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&**self, f)
    }
}

/// The resolver for `String`.
pub struct StringResolver(usize);

impl Archive for String {
    type Archived = ArchivedString;
    type Resolver = StringResolver;

    fn resolve(&self, pos: usize, resolver: StringResolver) -> Self::Archived {
        ArchivedString(self.as_str().resolve_ref(pos, resolver.0))
    }
}

impl<S: Serializer + ?Sized> Serialize<S> for String {
    fn serialize(&self, serializer: &mut S) -> Result<Self::Resolver, S::Error> {
        Ok(StringResolver(self.as_str().serialize_ref(serializer)?))
    }
}

impl<C: ?Sized> Deserialize<String, C> for Archived<String> {
    fn deserialize(&self, _: &mut C) -> String {
        self.as_str().to_string()
    }
}

/// An archived [`Box`].
///
/// This is a thin wrapper around the reference type for whatever type was
/// archived.
#[derive(Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct ArchivedBox<T>(T);

impl<T: DerefMut> ArchivedBox<T> {
    /// Gets the value of this archived box as a pinned mutable reference.
    pub fn get_pin(self: Pin<&mut Self>) -> Pin<&mut <T as Deref>::Target> {
        unsafe { self.map_unchecked_mut(|s| &mut **s) }
    }
}

impl<T: Deref> Deref for ArchivedBox<T> {
    type Target = T::Target;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<T: DerefMut> DerefMut for ArchivedBox<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

impl<T: Deref<Target = U>, U: PartialEq<V> + ?Sized, V: ?Sized> PartialEq<Box<V>>
    for ArchivedBox<T>
{
    fn eq(&self, other: &Box<V>) -> bool {
        self.deref().eq(other.deref())
    }
}

/// The resolver for `Box`.
pub struct BoxResolver(usize);

impl<T: ArchiveRef + ?Sized> Archive for Box<T> {
    type Archived = ArchivedBox<Reference<T>>;
    type Resolver = BoxResolver;

    fn resolve(&self, pos: usize, resolver: BoxResolver) -> Self::Archived {
        ArchivedBox(self.as_ref().resolve_ref(pos, resolver.0))
    }
}

impl<T: SerializeRef<S> + ?Sized, S: Serializer + ?Sized> Serialize<S> for Box<T> {
    fn serialize(&self, serializer: &mut S) -> Result<Self::Resolver, S::Error> {
        Ok(BoxResolver(self.as_ref().serialize_ref(serializer)?))
    }
}

impl<T: ArchiveRef + ?Sized, D: AllocDeserializer + ?Sized> Deserialize<Box<T>, D> for Archived<Box<T>>
where
    Reference<T>: DeserializeRef<T, D>,
{
    fn deserialize(&self, deserializer: &mut D) -> Box<T> {
        unsafe { Box::from_raw(self.0.deserialize_ref(deserializer)) }
    }
}

impl<T: Archive> ArchiveRef for [T] {
    type Archived = [T::Archived];
    type Reference = ArchivedSlice<T::Archived>;

    fn resolve_ref(&self, from: usize, to: usize) -> Self::Reference {
        unsafe { ArchivedSlice::new(from, to, self.len()) }
    }
}

impl<T: Serialize<S>, S: Serializer + ?Sized> SerializeRef<S> for [T] {
    fn serialize_ref(&self, serializer: &mut S) -> Result<usize, S::Error> {
        if !self.is_empty() {
            let mut resolvers = Vec::with_capacity(self.len());
            for value in self {
                resolvers.push(value.serialize(serializer)?);
            }
            let result = serializer.align_for::<T::Archived>()?;
            unsafe {
                for (i, resolver) in resolvers.drain(..).enumerate() {
                    serializer.resolve_aligned(&self[i], resolver)?;
                }
            }
            Ok(result)
        } else {
            Ok(0)
        }
    }
}

impl<T: Archive, D: AllocDeserializer + ?Sized> DeserializeRef<[T], D> for <[T] as ArchiveRef>::Reference
where
    T::Archived: Deserialize<T, D>,
{
    unsafe fn deserialize_ref(&self, deserializer: &mut D) -> *mut [T] {
        let result = deserializer.alloc(alloc::Layout::array::<T>(self.len()).unwrap()).cast::<T>();
        for i in 0..self.len() {
            result.add(i).write(self[i].deserialize(deserializer));
        }
        slice::from_raw_parts_mut(result, self.len())
    }
}

/// An archived [`Vec`].
///
/// Uses [`ArchivedSlice`](crate::core_impl::ArchivedSlice) under the hood.
#[derive(Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct ArchivedVec<T>(T);

impl<T: DerefMut> ArchivedVec<T> {
    /// Gets the element at the given index ot this archived vec as a pinned
    /// mutable reference.
    pub fn index_pin<I>(self: Pin<&mut Self>, index: I) -> Pin<&mut <T::Target as Index<I>>::Output>
    where
        T::Target: IndexMut<I>,
    {
        unsafe { self.map_unchecked_mut(|s| &mut (**s)[index]) }
    }
}

impl<T: Deref> Deref for ArchivedVec<T> {
    type Target = T::Target;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<T: DerefMut> DerefMut for ArchivedVec<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

/// The resolver for `Vec`.
pub struct VecResolver(usize);

impl<T: Archive> Archive for Vec<T> {
    type Archived = ArchivedVec<Reference<[T]>>;
    type Resolver = VecResolver;

    fn resolve(&self, pos: usize, resolver: VecResolver) -> Self::Archived {
        ArchivedVec(self.as_slice().resolve_ref(pos, resolver.0))
    }
}

impl<T: Serialize<S>, S: Serializer + ?Sized> Serialize<S> for Vec<T> {
    fn serialize(&self, serializer: &mut S) -> Result<Self::Resolver, S::Error> {
        Ok(VecResolver(self.as_slice().serialize_ref(serializer)?))
    }
}

impl<T: Archive, D: ?Sized> Deserialize<Vec<T>, D> for Archived<Vec<T>>
where
    T::Archived: Deserialize<T, D>,
{
    fn deserialize(&self, deserializer: &mut D) -> Vec<T> {
        let mut result = Vec::with_capacity(self.len());
        for i in self.iter() {
            result.push(i.deserialize(deserializer));
        }
        result
    }
}

impl<T: Deref<Target = [U]>, U: PartialEq<V>, V> PartialEq<Vec<V>> for ArchivedVec<T> {
    fn eq(&self, other: &Vec<V>) -> bool {
        self.deref().eq(other.deref())
    }
}
