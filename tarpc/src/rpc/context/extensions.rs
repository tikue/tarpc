// Copyright 2020 Google LLC
//
// Use of this source code is governed by an MIT-style
// license that can be found in the LICENSE file or at
// https://opensource.org/licenses/MIT.

//! Largely copied from [http](https://docs.rs/http/0.2.1/src/http/extensions.rs.html), except that
//! this one is `Clone`.

use static_assertions::assert_impl_all;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::{BuildHasherDefault, Hasher};

/// A type map of protocol extensions.
///
/// `Extensions` can be used by `Request` and `Response` to store
/// extra data derived from the underlying protocol.
#[derive(Clone, Debug, Default)]
pub struct Extensions {
    // If extensions are never used, no need to carry around an empty HashMap.
    // That's 3 words. Instead, this is only 1 word.
    map: Option<Box<ExtensionMap>>,
}

type ExtensionMap = HashMap<TypeId, Box<dyn Extension>, BuildHasherDefault<IdHasher>>;

/// An [`Any`] that is restricted to types that are Clone, Debug, Send, Sync, and 'static.
pub trait Extension: Any + Debug + Send + Sync + 'static {
    /// Returns a clone of itself.
    fn clone(&self) -> Box<dyn Extension>;
    /// Upcasts to `Box<dyn Any + 'static>`.
    fn into_any(self: Box<Self>) -> Box<dyn Any + 'static>;
    /// Upcasts to `&(dyn Any + 'static)`.
    fn as_any(&self) -> &(dyn Any + 'static);
    /// Upcasts to `&mut (dyn Any + 'static)`.
    fn as_mut_any(&mut self) -> &mut (dyn Any + 'static);
}

impl<T: Any + Clone + Debug + Send + Sync + 'static> Extension for T {
    fn clone(&self) -> Box<dyn Extension> {
        Box::new(Clone::clone(self))
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any + 'static> {
        self
    }

    fn as_any(&self) -> &(dyn Any + 'static) {
        self
    }

    fn as_mut_any(&mut self) -> &mut (dyn Any + 'static) {
        self
    }
}

impl Clone for Box<dyn Extension> {
    fn clone(&self) -> Self {
        Extension::clone(self.as_ref())
    }
}

trait Downcast: Extension {
    fn downcast<T: 'static>(self: Box<Self>) -> Result<Box<T>, Box<dyn Any + 'static>> {
        self.into_any().downcast()
    }
}

assert_impl_all!(Extensions: Send, Sync);

impl Extensions {
    /// Create an empty `Extensions`.
    #[inline]
    pub fn new() -> Extensions {
        Extensions { map: None }
    }

    /// Insert a type into this `Extensions`.
    ///
    /// If an extension of this type already existed, it will be returned.
    ///
    /// # Example
    ///
    /// ```
    /// # use tarpc::context::extensions::Extensions;
    /// let mut ext = Extensions::new();
    /// assert!(ext.insert(5i32).is_none());
    /// assert!(ext.insert(4u8).is_none());
    /// assert_eq!(ext.insert(9i32), Some(5i32));
    /// ```
    pub fn insert<T: Extension>(&mut self, val: T) -> Option<T> {
        self.map
            .get_or_insert_with(|| Box::new(HashMap::default()))
            .insert(TypeId::of::<T>(), Box::new(val))
            .and_then(|boxed| boxed.into_any().downcast().ok().map(|boxed| *boxed))
    }

    /// Get a reference to a type previously inserted into this `Extensions`.
    ///
    /// # Example
    ///
    /// ```
    /// # use tarpc::context::extensions::Extensions;
    /// let mut ext = Extensions::new();
    /// assert!(ext.get::<i32>().is_none());
    /// ext.insert(5i32);
    ///
    /// assert_eq!(ext.get::<i32>(), Some(&5i32));
    /// ```
    pub fn get<T: Extension>(&self) -> Option<&T> {
        self.map
            .as_ref()
            .and_then(|map| map.get(&TypeId::of::<T>()))
            .and_then(|boxed| boxed.as_ref().as_any().downcast_ref())
    }

    /// Get a mutable reference to a type previously inserted intoo this `Extensions`.
    ///
    /// # Example
    ///
    /// ```
    /// # use tarpc::context::extensions::Extensions;
    /// let mut ext = Extensions::new();
    /// ext.insert(String::from("Hello"));
    /// ext.get_mut::<String>().unwrap().push_str(" World");
    ///
    /// assert_eq!(ext.get::<String>().unwrap(), "Hello World");
    /// ```
    pub fn get_mut<T: Extension>(&mut self) -> Option<&mut T> {
        self.map
            .as_mut()
            .and_then(|map| map.get_mut(&TypeId::of::<T>()))
            .and_then(|boxed| boxed.as_mut().as_mut_any().downcast_mut())
    }

    /// Remove a type from this `Extensions`.
    ///
    /// If an extension of this type already existed, it will be returned.
    ///
    /// # Example
    ///
    /// ```
    /// # use tarpc::context::extensions::Extensions;
    /// let mut ext = Extensions::new();
    /// ext.insert(5i32);
    /// assert_eq!(ext.remove::<i32>(), Some(5i32));
    /// assert!(ext.get::<i32>().is_none());
    /// ```
    pub fn remove<T: Extension>(&mut self) -> Option<T> {
        self.map
            .as_mut()
            .and_then(|map| map.remove(&TypeId::of::<T>()))
            .and_then(|boxed| boxed.into_any().downcast().ok().map(|boxed| *boxed))
    }

    /// Clear the `Extensions` of all inserted extensions.
    ///
    /// # Example
    ///
    /// ```
    /// # use tarpc::context::extensions::Extensions;
    /// let mut ext = Extensions::new();
    /// ext.insert(5i32);
    /// ext.clear();
    ///
    /// assert!(ext.get::<i32>().is_none());
    /// ```
    #[inline]
    pub fn clear(&mut self) {
        if let Some(ref mut map) = self.map {
            map.clear();
        }
    }
}

// With TypeIds as keys, there's no need to hash them. They are already hashes
// themselves, coming from the compiler. The IdHasher just holds the u64 of
// the TypeId, and then returns it, instead of doing any bit fiddling.
#[derive(Default)]
struct IdHasher(u64);

impl Hasher for IdHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, _: &[u8]) {
        unreachable!("TypeId calls write_u64");
    }

    #[inline]
    fn write_u64(&mut self, id: u64) {
        self.0 = id;
    }
}

#[test]
fn test_extensions() {
    #[derive(Clone, Debug, PartialEq)]
    struct MyType(i32);

    let mut extensions = Extensions::new();

    assert_eq!(extensions.insert(5i32), None);
    assert_eq!(extensions.insert(MyType(10)), None);

    assert_eq!(extensions.get(), Some(&5i32));
    assert_eq!(extensions.get_mut(), Some(&mut 5i32));

    assert_eq!(extensions.remove::<i32>(), Some(5i32));
    assert!(extensions.get::<i32>().is_none());

    assert_eq!(extensions.get::<bool>(), None);
    assert_eq!(extensions.get(), Some(&MyType(10)));
}
