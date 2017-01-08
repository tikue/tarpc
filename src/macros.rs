// Copyright 2016 Google Inc. All Rights Reserved.
//
// Licensed under the MIT License, <LICENSE or http://opensource.org/licenses/MIT>.
// This file may not be copied, modified, or distributed except according to those terms.

#[doc(hidden)]
#[macro_export]
macro_rules! as_item {
    ($i:item) => {$i};
}

/// Creates an enum where each variant contains a `Future`. The created enum impls `Future`.
/// Useful when a fn needs to return possibly many different types of futures.
#[macro_export]
macro_rules! future_enum {
    {
        $(#[$attr:meta])*
        pub enum $name:ident<$($tp:ident),*> {
            $(#[$attrv:meta])*
            $($variant:ident($inner:ty)),*
        }
    } => {
        future_enum! {
            $(#[$attr:meta])*
            (pub) enum $name<$($tp),*> {
                $(#[$attrv])*
                $($variant($inner)),*
            }
        }
    };
    {
        $(#[$attr:meta])*
        enum $name:ident<$($tp:ident),*> {
            $(#[$attrv:meta])*
            $($variant:ident($inner:ty)),*
        }
    } => {
        future_enum! {
            $(#[$attr:meta])*
            () enum $name<$($tp),*> {
                $(#[$attrv])*
                $($variant($inner)),*
            }
        }
    };
    {
        $(#[$attr:meta])*
        ($($vis:tt)*) enum $name:ident<$($tp:ident),*> {
            $(#[$attrv:meta])*
            $($variant:ident($inner:ty)),*
        }
    } => {
        $(#[$attr])*
        as_item! {
            $($vis)* enum $name<$($tp),*> {
                $(#[$attrv])*
                $($variant($inner)),*
            }
        }

        #[allow(non_camel_case_types)]
        impl<__future_enum_T, __future_enum_E, $($tp),*> $crate::futures::Future for $name<$($tp),*>
            where __future_enum_T: Send + 'static,
                  $($inner: $crate::futures::Future<Item=__future_enum_T, Error=__future_enum_E>),*
        {
            type Item = __future_enum_T;
            type Error = __future_enum_E;

            fn poll(&mut self) -> $crate::futures::Poll<Self::Item, Self::Error> {
                match *self {
                    $($name::$variant(ref mut f) => $crate::futures::Future::poll(f)),*
                }
            }
        }
    }
}

#[doc(hidden)]
#[macro_export]
macro_rules! impl_serialize {
    ($impler:ident, { $($lifetime:tt)* }, $(@($name:ident $n:expr))* -- #($_n:expr) ) => {
        as_item! {
            impl$($lifetime)* $crate::serde::Serialize for $impler$($lifetime)* {
                fn serialize<S>(&self, __impl_serialize_serializer: &mut S)
                    -> ::std::result::Result<(), S::Error>
                    where S: $crate::serde::Serializer
                {
                    match *self {
                        $(
                            $impler::$name(ref __impl_serialize_field) =>
                                $crate::serde::Serializer::serialize_newtype_variant(
                                    __impl_serialize_serializer,
                                    stringify!($impler),
                                    $n,
                                    stringify!($name),
                                    __impl_serialize_field,
                                )
                        ),*
                    }
                }
            }
        }
    };
    // All args are wrapped in a tuple so we can use the newtype variant for each one.
    ($impler:ident,
     { $($lifetime:tt)* },
     $(@$finished:tt)*
     -- #($n:expr) $name:ident($field:ty) $($req:tt)*) =>
    (
        impl_serialize!($impler,
                        { $($lifetime)* },
                        $(@$finished)* @($name $n)
                        -- #($n + 1) $($req)*);
    );
    // Entry
    ($impler:ident,
     { $($lifetime:tt)* },
     $($started:tt)*) => (impl_serialize!($impler, { $($lifetime)* }, -- #(0) $($started)*););
}

#[doc(hidden)]
#[macro_export]
macro_rules! impl_deserialize {
    ($impler:ident, $(@($name:ident $n:expr))* -- #($_n:expr) ) => (
        impl $crate::serde::Deserialize for $impler {
            #[allow(non_camel_case_types)]
            fn deserialize<__impl_deserialize_D>(
                __impl_deserialize_deserializer: &mut __impl_deserialize_D)
                -> ::std::result::Result<$impler, __impl_deserialize_D::Error>
                where __impl_deserialize_D: $crate::serde::Deserializer
            {
                #[allow(non_camel_case_types, unused)]
                enum __impl_deserialize_Field {
                    $($name),*
                }

                impl $crate::serde::Deserialize for __impl_deserialize_Field {
                    fn deserialize<D>(__impl_deserialize_deserializer: &mut D)
                        -> ::std::result::Result<__impl_deserialize_Field, D::Error>
                        where D: $crate::serde::Deserializer
                    {
                        struct __impl_deserialize_FieldVisitor;
                        impl $crate::serde::de::Visitor for __impl_deserialize_FieldVisitor {
                            type Value = __impl_deserialize_Field;

                            fn visit_usize<E>(&mut self, __impl_deserialize_value: usize)
                                -> ::std::result::Result<__impl_deserialize_Field, E>
                                where E: $crate::serde::de::Error,
                            {
                                $(
                                    if __impl_deserialize_value == $n {
                                        return ::std::result::Result::Ok(
                                            __impl_deserialize_Field::$name);
                                    }
                                )*
                                ::std::result::Result::Err(
                                    $crate::serde::de::Error::custom(
                                        format!("No variants have a value of {}!",
                                                __impl_deserialize_value))
                                )
                            }
                        }
                        __impl_deserialize_deserializer.deserialize_struct_field(
                            __impl_deserialize_FieldVisitor)
                    }
                }

                struct Visitor;
                impl $crate::serde::de::EnumVisitor for Visitor {
                    type Value = $impler;

                    fn visit<V>(&mut self, mut __tarpc_enum_visitor: V)
                        -> ::std::result::Result<$impler, V::Error>
                        where V: $crate::serde::de::VariantVisitor
                    {
                        match __tarpc_enum_visitor.visit_variant()? {
                            $(
                                __impl_deserialize_Field::$name => {
                                    ::std::result::Result::Ok(
                                        $impler::$name(__tarpc_enum_visitor.visit_newtype()?))
                                }
                            ),*
                        }
                    }
                }
                const __TARPC_VARIANTS: &'static [&'static str] = &[
                    $(
                        stringify!($name)
                    ),*
                ];
                __impl_deserialize_deserializer.deserialize_enum(
                    stringify!($impler), __TARPC_VARIANTS, Visitor)
            }
        }
    );
    // All args are wrapped in a tuple so we can use the newtype variant for each one.
    ($impler:ident, $(@$finished:tt)* -- #($n:expr) $name:ident($field:ty) $($req:tt)*) => (
        impl_deserialize!($impler, $(@$finished)* @($name $n) -- #($n + 1) $($req)*);
    );
    // Entry
    ($impler:ident, $($started:tt)*) => (impl_deserialize!($impler, -- #(0) $($started)*););
}

#[doc(hidden)]
#[macro_export]
macro_rules! __tarpc_common {
    (
        $(
            $(#[$attr:meta])*
            rpc $fn_name:ident ( $( $arg:ident : $in_:ty ),* ) -> $out:ty | $error:ty;
        )*
    ) => {
        #[allow(non_camel_case_types, unused)]
        #[derive(Debug)]
        enum __tarpc_service_Request {
            NotIrrefutable(()),
            $(
                $fn_name(( $($in_,)* ))
            ),*
        }

        impl_deserialize!(__tarpc_service_Request, NotIrrefutable(()) $($fn_name(($($in_),*)))*);
        impl_serialize!(__tarpc_service_Request, {}, NotIrrefutable(()) $($fn_name(($($in_),*)))*);

        #[allow(non_camel_case_types, unused)]
        #[derive(Debug)]
        enum __tarpc_service_Response {
            NotIrrefutable(()),
            $(
                $fn_name($out)
            ),*
        }

        impl_deserialize!(__tarpc_service_Response, NotIrrefutable(()) $($fn_name($out))*);
        impl_serialize!(__tarpc_service_Response, {}, NotIrrefutable(()) $($fn_name($out))*);

        #[allow(non_camel_case_types, unused)]
        #[derive(Debug)]
        enum __tarpc_service_Error {
            NotIrrefutable(()),
            $(
                $fn_name($error)
            ),*
        }

        impl_deserialize!(__tarpc_service_Error, NotIrrefutable(()) $($fn_name($error))*);
        impl_serialize!(__tarpc_service_Error, {}, NotIrrefutable(()) $($fn_name($error))*);

/// Defines the `Future` RPC service. Implementors must be `Clone`, `Send`, and `'static`,
/// as required by `tokio_proto::NewService`. This is required so that the service can be used
/// to respond to multiple requests concurrently.
        pub trait FutureService:
            ::std::marker::Send +
            ::std::clone::Clone +
            'static
        {
            $(

                snake_to_camel! {
                    /// The type of future returned by `{}`.
                    type $fn_name: $crate::futures::Future<Item=$out, Error=$error>;
                }

                $(#[$attr])*
                fn $fn_name(&mut self, $($arg:$in_),*) -> ty_snake_to_camel!(Self::$fn_name);
            )*
        }

/// Defines the blocking RPC service. Must be `Clone`, `Send`, and `'static`,
/// as required by `tokio_proto::NewService`. This is required so that the service can be used
/// to respond to multiple requests concurrently.
        pub trait SyncService:
            ::std::marker::Send +
            ::std::clone::Clone +
            'static
        {
            $(
                $(#[$attr])*
                fn $fn_name(&mut self, $($arg:$in_),*) -> ::std::result::Result<$out, $error>;
            )*
        }

        impl<A> FutureServiceExt for A where A: FutureService {}
        impl<S> SyncServiceExt for S where S: SyncService {}

        #[allow(unused)]
        #[derive(Clone, Debug)]
        /// The client stub that makes RPC calls to the server. Exposes a blocking interface.
        pub struct SyncClient(FutureClient);

        impl SyncClient {
            $(
                #[allow(unused)]
                $(#[$attr])*
                pub fn $fn_name(&mut self, $($arg: $in_),*)
                    -> ::std::result::Result<$out, $crate::Error<$error>>
                {
                    let rpc = (self.0).$fn_name($($arg),*);
                    $crate::futures::Future::wait(rpc)
                }
            )*
        }

        #[allow(non_camel_case_types)]
        type __tarpc_service_Client =
            $crate::Client<__tarpc_service_Request,
                           __tarpc_service_Response,
                           __tarpc_service_Error>;

        #[allow(non_camel_case_types)]
        /// Implementation detail: Pending connection.
        pub struct __tarpc_service_ConnectFuture<T> {
            inner: $crate::futures::Map<$crate::ConnectFuture<__tarpc_service_Request,
                                                              __tarpc_service_Response,
                                                              __tarpc_service_Error>,
                                        fn(__tarpc_service_Client) -> T>,
        }

        impl<T> $crate::futures::Future for __tarpc_service_ConnectFuture<T> {
            type Item = T;
            type Error = ::std::io::Error;

            fn poll(&mut self) -> $crate::futures::Poll<Self::Item, Self::Error> {
                $crate::futures::Future::poll(&mut self.inner)
            }
        }

        #[allow(non_camel_case_types)]
        /// Implementation detail: Pending connection.
        pub struct __tarpc_service_ConnectWithFuture<'a, T> {
            inner: $crate::futures::Map<$crate::ConnectWithFuture<'a,
                                                                  __tarpc_service_Request,
                                                                  __tarpc_service_Response,
                                                                  __tarpc_service_Error>,
                                        fn(__tarpc_service_Client) -> T>,
        }

        impl<'a, T> $crate::futures::Future for __tarpc_service_ConnectWithFuture<'a, T> {
            type Item = T;
            type Error = ::std::io::Error;

            fn poll(&mut self) -> $crate::futures::Poll<Self::Item, Self::Error> {
                $crate::futures::Future::poll(&mut self.inner)
            }
        }

        #[allow(unused)]
        #[derive(Clone, Debug)]
        /// The client stub that makes RPC calls to the server. Exposes a Future interface.
        pub struct FutureClient(__tarpc_service_Client);

        impl FutureClient {
            $(
                #[allow(unused)]
                $(#[$attr])*
                pub fn $fn_name(&mut self, $($arg: $in_),*)
                    -> impl $crate::futures::Future<Item=$out, Error=$crate::Error<$error>>
                    + 'static
                {
                    let __tarpc_service_req = __tarpc_service_Request::$fn_name(($($arg,)*));
                    let __tarpc_service_fut =
                        $crate::tokio_service::Service::call(&mut self.0, __tarpc_service_req);
                    $crate::futures::Future::then(__tarpc_service_fut,
                                                  move |__tarpc_service_msg| {
                        match __tarpc_service_msg? {
                            ::std::result::Result::Ok(__tarpc_service_msg) => {
                                if let __tarpc_service_Response::$fn_name(__tarpc_service_msg) =
                                    __tarpc_service_msg
                                {
                                    ::std::result::Result::Ok(__tarpc_service_msg)
                                } else {
                                   unreachable!()
                                }
                            }
                            ::std::result::Result::Err(__tarpc_service_err) => {
                                ::std::result::Result::Err(match __tarpc_service_err {
                                    $crate::Error::App(__tarpc_service_err) => {
                                        if let __tarpc_service_Error::$fn_name(
                                            __tarpc_service_err) = __tarpc_service_err
                                        {
                                            $crate::Error::App(__tarpc_service_err)
                                        } else {
                                            unreachable!()
                                        }
                                    }
                                    $crate::Error::ServerDeserialize(__tarpc_service_err) => {
                                        $crate::Error::ServerDeserialize(__tarpc_service_err)
                                    }
                                    $crate::Error::ServerSerialize(__tarpc_service_err) => {
                                        $crate::Error::ServerSerialize(__tarpc_service_err)
                                    }
                                    $crate::Error::ClientDeserialize(__tarpc_service_err) => {
                                        $crate::Error::ClientDeserialize(__tarpc_service_err)
                                    }
                                    $crate::Error::ClientSerialize(__tarpc_service_err) => {
                                        $crate::Error::ClientSerialize(__tarpc_service_err)
                                    }
                                    $crate::Error::Io(__tarpc_service_error) => {
                                        $crate::Error::Io(__tarpc_service_error)
                                    }
                                })
                            }
                        }
                    })
                }
            )*
        }
    }
}

#[doc(hidden)]
#[macro_export]
macro_rules! __tarpc_common_fut_listen_with {
    (
        $(
            $(#[$attr:meta])*
            rpc $fn_name:ident ( $( $arg:ident : $in_:ty ),* ) -> $out:ty | $error:ty;
        )*
    ) => {
        #[allow(non_camel_case_types)]
        #[derive(Clone)]
        struct __tarpc_service_AsyncServer<S>(S);

        impl<S> ::std::fmt::Debug for __tarpc_service_AsyncServer<S> {
            fn fmt(&self, fmt: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                write!(fmt, "__tarpc_service_AsyncServer {{ .. }}")
            }
        }

        #[allow(non_camel_case_types)]
        type __tarpc_service_Future =
            $crate::futures::Finished<$crate::Response<__tarpc_service_Response,
                                                       __tarpc_service_Error>,
                                      ::std::io::Error>;

        #[allow(non_camel_case_types)]
        enum __tarpc_service_FutureReply<__tarpc_service_S: FutureService> {
            DeserializeError(__tarpc_service_Future),
            $($fn_name(
                    $crate::futures::Then<ty_snake_to_camel!(__tarpc_service_S::$fn_name),
                                          __tarpc_service_Future,
                                          fn(::std::result::Result<$out, $error>)
                                              -> __tarpc_service_Future>)),*
        }

        impl<S: FutureService> $crate::futures::Future for __tarpc_service_FutureReply<S> {
            type Item = $crate::Response<__tarpc_service_Response, __tarpc_service_Error>;

            type Error = ::std::io::Error;

            fn poll(&mut self) -> $crate::futures::Poll<Self::Item, Self::Error> {
                match *self {
                    __tarpc_service_FutureReply::DeserializeError(
                        ref mut __tarpc_service_future) =>
                    {
                        $crate::futures::Future::poll(__tarpc_service_future)
                    }
                    $(
                        __tarpc_service_FutureReply::$fn_name(
                            ref mut __tarpc_service_future) =>
                        {
                            $crate::futures::Future::poll(__tarpc_service_future)
                        }
                    ),*
                }
            }
        }


        #[allow(non_camel_case_types)]
        impl<__tarpc_service_S> $crate::tokio_service::Service
            for __tarpc_service_AsyncServer<__tarpc_service_S>
            where __tarpc_service_S: FutureService
        {
            type Request = ::std::result::Result<__tarpc_service_Request,
                                                 $crate::bincode::serde::DeserializeError>;
            type Response = $crate::Response<__tarpc_service_Response,
                                             __tarpc_service_Error>;
            type Error = ::std::io::Error;
            type Future = __tarpc_service_FutureReply<__tarpc_service_S>;

            fn call(&mut self, __tarpc_service_request: Self::Request) -> Self::Future {
                let __tarpc_service_request = match __tarpc_service_request {
                    Ok(__tarpc_service_request) => __tarpc_service_request,
                    Err(__tarpc_service_deserialize_err) => {
                        return __tarpc_service_FutureReply::DeserializeError(
                            $crate::futures::finished(
                                ::std::result::Result::Err(
                                    $crate::WireError::ServerDeserialize(
                                        ::std::string::ToString::to_string(
                                            &__tarpc_service_deserialize_err)))));
                    }
                };
                match __tarpc_service_request {
                    __tarpc_service_Request::NotIrrefutable(()) => unreachable!(),
                    $(
                        __tarpc_service_Request::$fn_name(( $($arg,)* )) => {
                            fn __tarpc_service_wrap(
                                __tarpc_service_response:
                                    ::std::result::Result<$out, $error>)
                                -> __tarpc_service_Future
                            {
                                $crate::futures::finished(
                                    __tarpc_service_response
                                        .map(__tarpc_service_Response::$fn_name)
                                        .map_err(|__tarpc_service_error| {
                                            $crate::WireError::App(
                                                __tarpc_service_Error::$fn_name(
                                                    __tarpc_service_error))
                                        })
                                )
                            }
                            return __tarpc_service_FutureReply::$fn_name(
                                $crate::futures::Future::then(
                                        FutureService::$fn_name(&mut self.0, $($arg),*),
                                        __tarpc_service_wrap));
                        }
                    )*
                }
            }
        }
    }
}

#[doc(hidden)]
#[macro_export]
macro_rules! __tarpc_common_sync_listen_with {
    (
        $(
            $(#[$attr:meta])*
            rpc $fn_name:ident ( $( $arg:ident : $in_:ty ),* ) -> $out:ty | $error:ty;
        )*
    ) => {
        #[derive(Clone)]
        struct __SyncServer<S> {
            service: S,
        }

        #[allow(non_camel_case_types)]
        impl<__tarpc_service_S> FutureService for __SyncServer<__tarpc_service_S>
            where __tarpc_service_S: SyncService
        {
            $(
                impl_snake_to_camel! {
                    type $fn_name =
                        $crate::futures::Flatten<
                            $crate::futures::MapErr<
                                $crate::futures::Oneshot<
                                    $crate::futures::Done<$out, $error>>,
                                fn($crate::futures::Canceled) -> $error>>;
                }
                fn $fn_name(&mut self, $($arg:$in_),*) -> ty_snake_to_camel!(Self::$fn_name) {
                    fn unimplemented(_: $crate::futures::Canceled) -> $error {
                        // TODO(tikue): what do do if SyncService panics?
                        unimplemented!()
                    }
                    let (__tarpc_service_complete, __tarpc_service_promise) =
                        $crate::futures::oneshot();
                    let mut __tarpc_service_service = self.clone();
                    const UNIMPLEMENTED: fn($crate::futures::Canceled) -> $error =
                        unimplemented;
                    ::std::thread::spawn(move || {
                        let __tarpc_service_reply = SyncService::$fn_name(
                            &mut __tarpc_service_service.service, $($arg),*);
                        __tarpc_service_complete.complete(
                            $crate::futures::IntoFuture::into_future(
                                __tarpc_service_reply));
                    });
                    let __tarpc_service_promise =
                        $crate::futures::Future::map_err(
                            __tarpc_service_promise, UNIMPLEMENTED);
                    $crate::futures::Future::flatten(__tarpc_service_promise)
                }
            )*
        }
    }
}

/// The main macro that creates RPC services.
///
/// Rpc methods are specified, mirroring trait syntax:
///
/// ```
/// # #![feature(conservative_impl_trait, plugin)]
/// # #![plugin(tarpc_plugins)]
/// # #[macro_use] extern crate tarpc;
/// # fn main() {}
/// # service! {
/// /// Say hello
/// rpc hello(name: String) -> String;
/// # }
/// ```
///
/// Attributes can be attached to each rpc. These attributes
/// will then be attached to the generated service traits'
/// corresponding `fn`s, as well as to the client stubs' RPCs.
///
/// The following items are expanded in the enclosing module:
///
/// * `FutureService` -- the trait defining the RPC service via a `Future` API.
/// * `SyncService` -- a service trait that provides a synchronous API for when
///                    spawning a thread per request is acceptable.
/// * `FutureServiceExt` -- provides the methods for starting a service. There is an umbrella impl
///                         for all implers of `FutureService`. It's a separate trait to prevent
///                         name collisions with RPCs.
/// * `SyncServiceExt` -- same as `FutureServiceExt` but for `SyncService`.
/// * `FutureClient` -- a client whose RPCs return `Future`s.
/// * `SyncClient` -- a client whose RPCs block until the reply is available. Easiest
///                   interface to use, as it looks the same as a regular function call.
///
#[cfg(not(feature = "tls"))]
#[macro_export]
macro_rules! service {
// Entry point
    (
        $(
            $(#[$attr:meta])*
            rpc $fn_name:ident( $( $arg:ident : $in_:ty ),* ) $(-> $out:ty)* $(| $error:ty)*;
        )*
    ) => {
        service! {{
            $(
                $(#[$attr])*
                rpc $fn_name( $( $arg : $in_ ),* ) $(-> $out)* $(| $error)*;
            )*
        }}
    };
// Pattern for when the next rpc has an implicit unit return type and no error type.
    (
        {
            $(#[$attr:meta])*
            rpc $fn_name:ident( $( $arg:ident : $in_:ty ),* );

            $( $unexpanded:tt )*
        }
        $( $expanded:tt )*
    ) => {
        service! {
            { $( $unexpanded )* }

            $( $expanded )*

            $(#[$attr])*
            rpc $fn_name( $( $arg : $in_ ),* ) -> () | $crate::util::Never;
        }
    };
// Pattern for when the next rpc has an explicit return type and no error type.
    (
        {
            $(#[$attr:meta])*
            rpc $fn_name:ident( $( $arg:ident : $in_:ty ),* ) -> $out:ty;

            $( $unexpanded:tt )*
        }
        $( $expanded:tt )*
    ) => {
        service! {
            { $( $unexpanded )* }

            $( $expanded )*

            $(#[$attr])*
            rpc $fn_name( $( $arg : $in_ ),* ) -> $out | $crate::util::Never;
        }
    };
// Pattern for when the next rpc has an implicit unit return type and an explicit error type.
    (
        {
            $(#[$attr:meta])*
            rpc $fn_name:ident( $( $arg:ident : $in_:ty ),* ) | $error:ty;

            $( $unexpanded:tt )*
        }
        $( $expanded:tt )*
    ) => {
        service! {
            { $( $unexpanded )* }

            $( $expanded )*

            $(#[$attr])*
            rpc $fn_name( $( $arg : $in_ ),* ) -> () | $error;
        }
    };
// Pattern for when the next rpc has an explicit return type and an explicit error type.
    (
        {
            $(#[$attr:meta])*
            rpc $fn_name:ident( $( $arg:ident : $in_:ty ),* ) -> $out:ty | $error:ty;

            $( $unexpanded:tt )*
        }
        $( $expanded:tt )*
    ) => {
        service! {
            { $( $unexpanded )* }

            $( $expanded )*

            $(#[$attr])*
            rpc $fn_name( $( $arg : $in_ ),* ) -> $out | $error;
        }
    };
// Pattern for when all return types have been expanded
    (
        { } // none left to expand
        $(
            $(#[$attr:meta])*
            rpc $fn_name:ident ( $( $arg:ident : $in_:ty ),* ) -> $out:ty | $error:ty;
        )*
    ) => {

        __tarpc_common! {
            $(
                $(#[$attr])*
                rpc $fn_name ( $( $arg : $in_ ),* ) -> $out | $error;
             )*
        }

        /// Provides a function for starting the service. This is a separate trait from
        /// `FutureService` to prevent collisions with the names of RPCs.
        pub trait FutureServiceExt: FutureService {
            fn listen(self, addr: ::std::net::SocketAddr) -> $crate::ListenFuture
            {
                let (tx, rx) = $crate::futures::oneshot();
                $crate::future::REMOTE.spawn(move |handle|
                                     Ok(tx.complete(Self::listen_with(self,
                                                                      addr,
                                                                      handle.clone()))));
                $crate::ListenFuture::from_oneshot(rx)
            }

            /// Spawns the service, binding to the given address and running on
            /// the default tokio `Loop`.
            fn listen_with(self,
                           addr: ::std::net::SocketAddr,
                           handle: $crate::tokio_core::reactor::Handle)
                -> ::std::io::Result<::std::net::SocketAddr>
            {
                return $crate::listen_with(addr,
                                           move || Ok(__tarpc_service_AsyncServer(self.clone())),
                                           handle);

                __tarpc_common_fut_listen_with! {
                    $(
                        $(#[$attr])*
                        rpc $fn_name ( $( $arg : $in_ ),* ) -> $out | $error;
                     )*
                }
            }
        }

        /// Provides a function for starting the service. This is a separate trait from
        /// `SyncService` to prevent collisions with the names of RPCs.
        pub trait SyncServiceExt: SyncService {
            fn listen<L>(self, addr: L)
                -> ::std::io::Result<::std::net::SocketAddr>
                where L: ::std::net::ToSocketAddrs
            {
                let addr = $crate::util::FirstSocketAddr::try_first_socket_addr(&addr)?;
                let (tx, rx) = $crate::futures::oneshot();
                $crate::future::REMOTE.spawn(move |handle| Ok(tx.complete(Self::listen_with(self, addr, handle.clone()))));
                $crate::futures::Future::wait($crate::ListenFuture::from_oneshot(rx))
            }

            /// Spawns the service, binding to the given address and running on
            /// the default tokio `Loop`.
            fn listen_with<L>(self, addr: L, handle: $crate::tokio_core::reactor::Handle)
                -> ::std::io::Result<::std::net::SocketAddr>
                where L: ::std::net::ToSocketAddrs
            {
                let __tarpc_service_service = __SyncServer {
                    service: self,
                };
                return FutureServiceExt::listen_with(
                    __tarpc_service_service,
                    $crate::util::FirstSocketAddr::try_first_socket_addr(&addr)?,
                    handle);

                __tarpc_common_sync_listen_with! {
                    $(
                        $(#[$attr])*
                        rpc $fn_name ( $( $arg : $in_ ),* ) -> $out | $error;
                     )*
                }
            }
        }

        impl $crate::sync::Connect for SyncClient {
            fn connect<A>(addr: A) -> ::std::result::Result<Self, ::std::io::Error>
                where A: ::std::net::ToSocketAddrs,
            {
                let addr = $crate::util::FirstSocketAddr::try_first_socket_addr(&addr)?;
                let client = <FutureClient as $crate::future::Connect>::connect(&addr);
                let client = SyncClient($crate::futures::Future::wait(client)?);
                ::std::result::Result::Ok(client)
            }
        }

        impl<'a> $crate::future::Connect<'a> for FutureClient {
            type ConnectFut = __tarpc_service_ConnectFuture<Self>;
            type ConnectWithFut = __tarpc_service_ConnectWithFuture<'a, Self>;

            fn connect_remotely(__tarpc_service_addr: &::std::net::SocketAddr,
                                __tarpc_service_remote: &$crate::tokio_core::reactor::Remote)
                -> Self::ConnectFut
            {
                let client = <__tarpc_service_Client as $crate::future::Connect>::connect_remotely(
                    __tarpc_service_addr, __tarpc_service_remote);

                __tarpc_service_ConnectFuture {
                    inner: $crate::futures::Future::map(client, FutureClient)
                }
            }

            fn connect_with(__tarpc_service_addr: &::std::net::SocketAddr,
                            __tarpc_service_handle: &'a $crate::tokio_core::reactor::Handle)
                -> Self::ConnectWithFut
            {
                let client = <__tarpc_service_Client as $crate::future::Connect>::connect_with(
                    __tarpc_service_addr, __tarpc_service_handle);

                __tarpc_service_ConnectWithFuture {
                    inner: $crate::futures::Future::map(client, FutureClient)
                }
            }
        }
    }
}

/// The main macro that creates RPC services.
///
/// Rpc methods are specified, mirroring trait syntax:
///
/// ```
/// # #![feature(conservative_impl_trait, plugin)]
/// # #![plugin(tarpc_plugins)]
/// # #[macro_use] extern crate tarpc;
/// # fn main() {}
/// # service! {
/// /// Say hello
/// rpc hello(name: String) -> String;
/// # }
/// ```
///
/// Attributes can be attached to each rpc. These attributes
/// will then be attached to the generated service traits'
/// corresponding `fn`s, as well as to the client stubs' RPCs.
///
/// The following items are expanded in the enclosing module:
///
/// * `FutureService` -- the trait defining the RPC service via a `Future` API.
/// * `SyncService` -- a service trait that provides a synchronous API for when
///                    spawning a thread per request is acceptable.
/// * `FutureServiceExt` -- provides the methods for starting a service. There is an umbrella impl
///                         for all implers of `FutureService`. It's a separate trait to prevent
///                         name collisions with RPCs.
/// * `SyncServiceExt` -- same as `FutureServiceExt` but for `SyncService`.
/// * `FutureClient` -- a client whose RPCs return `Future`s.
/// * `SyncClient` -- a client whose RPCs block until the reply is available. Easiest
///                   interface to use, as it looks the same as a regular function call.
///
#[cfg(feature = "tls")]
#[macro_export]
macro_rules! service {
// Entry point
    (
        $(
            $(#[$attr:meta])*
            rpc $fn_name:ident( $( $arg:ident : $in_:ty ),* ) $(-> $out:ty)* $(| $error:ty)*;
        )*
    ) => {
        service! {{
            $(
                $(#[$attr])*
                rpc $fn_name( $( $arg : $in_ ),* ) $(-> $out)* $(| $error)*;
            )*
        }}
    };
// Pattern for when the next rpc has an implicit unit return type and no error type.
    (
        {
            $(#[$attr:meta])*
            rpc $fn_name:ident( $( $arg:ident : $in_:ty ),* );

            $( $unexpanded:tt )*
        }
        $( $expanded:tt )*
    ) => {
        service! {
            { $( $unexpanded )* }

            $( $expanded )*

            $(#[$attr])*
            rpc $fn_name( $( $arg : $in_ ),* ) -> () | $crate::util::Never;
        }
    };
// Pattern for when the next rpc has an explicit return type and no error type.
    (
        {
            $(#[$attr:meta])*
            rpc $fn_name:ident( $( $arg:ident : $in_:ty ),* ) -> $out:ty;

            $( $unexpanded:tt )*
        }
        $( $expanded:tt )*
    ) => {
        service! {
            { $( $unexpanded )* }

            $( $expanded )*

            $(#[$attr])*
            rpc $fn_name( $( $arg : $in_ ),* ) -> $out | $crate::util::Never;
        }
    };
// Pattern for when the next rpc has an implicit unit return type and an explicit error type.
    (
        {
            $(#[$attr:meta])*
            rpc $fn_name:ident( $( $arg:ident : $in_:ty ),* ) | $error:ty;

            $( $unexpanded:tt )*
        }
        $( $expanded:tt )*
    ) => {
        service! {
            { $( $unexpanded )* }

            $( $expanded )*

            $(#[$attr])*
            rpc $fn_name( $( $arg : $in_ ),* ) -> () | $error;
        }
    };
// Pattern for when the next rpc has an explicit return type and an explicit error type.
    (
        {
            $(#[$attr:meta])*
            rpc $fn_name:ident( $( $arg:ident : $in_:ty ),* ) -> $out:ty | $error:ty;

            $( $unexpanded:tt )*
        }
        $( $expanded:tt )*
    ) => {
        service! {
            { $( $unexpanded )* }

            $( $expanded )*

            $(#[$attr])*
            rpc $fn_name( $( $arg : $in_ ),* ) -> $out | $error;
        }
    };
// Pattern for when all return types have been expanded
    (
        { } // none left to expand
        $(
            $(#[$attr:meta])*
            rpc $fn_name:ident ( $( $arg:ident : $in_:ty ),* ) -> $out:ty | $error:ty;
        )*
    ) => {
        __tarpc_common! {
            $(
                $(#[$attr])*
                rpc $fn_name ( $( $arg : $in_ ),* ) -> $out | $error;
             )*
        }

        /// Provides a function for starting the service. This is a separate trait from
        /// `FutureService` to prevent collisions with the names of RPCs.
        pub trait FutureServiceExt: FutureService {
            fn listen(self, addr: ::std::net::SocketAddr, tls_acceptor: $crate::native_tls::TlsAcceptor) -> $crate::ListenFuture
            {
                let (tx, rx) = $crate::futures::oneshot();
                $crate::future::REMOTE.spawn(move |handle|
                                     Ok(tx.complete(Self::listen_with(self,
                                                                      addr,
                                                                      handle.clone(),
                                                                      tls_acceptor))));
                $crate::ListenFuture::from_oneshot(rx)
            }

            /// Spawns the service, binding to the given address and running on
            /// the default tokio `Loop`.
            fn listen_with(self,
                           addr: ::std::net::SocketAddr,
                           handle: $crate::tokio_core::reactor::Handle,
                           tls_acceptor: $crate::native_tls::TlsAcceptor)
                -> ::std::io::Result<::std::net::SocketAddr>
            {
                return $crate::listen_with(addr,
                                           move || Ok(__tarpc_service_AsyncServer(self.clone())),
                                           handle,
                                           tls_acceptor);

                __tarpc_common_fut_listen_with! {
                    $(
                        $(#[$attr])*
                        rpc $fn_name ( $( $arg : $in_ ),* ) -> $out | $error;
                     )*
                }

            }
        }

        /// Provides a function for starting the service. This is a separate trait from
        /// `SyncService` to prevent collisions with the names of RPCs.
        pub trait SyncServiceExt: SyncService {
            fn listen<L>(self, addr: L, tls_acceptor: $crate::native_tls::TlsAcceptor)
                -> ::std::io::Result<::std::net::SocketAddr>
                where L: ::std::net::ToSocketAddrs
            {
                let addr = $crate::util::FirstSocketAddr::try_first_socket_addr(&addr)?;
                let (tx, rx) = $crate::futures::oneshot();
                $crate::future::REMOTE.spawn(move |handle| Ok(tx.complete(Self::listen_with(self, addr, handle.clone(), tls_acceptor))));
                $crate::futures::Future::wait($crate::ListenFuture::from_oneshot(rx))
            }

            /// Spawns the service, binding to the given address and running on
            /// the default tokio `Loop`.
            fn listen_with<L>(self, addr: L, handle: $crate::tokio_core::reactor::Handle, tls_acceptor: $crate::native_tls::TlsAcceptor)
                -> ::std::io::Result<::std::net::SocketAddr>
                where L: ::std::net::ToSocketAddrs
            {
                let __tarpc_service_service = __SyncServer {
                    service: self,
                };
                return FutureServiceExt::listen_with(
                    __tarpc_service_service,
                    $crate::util::FirstSocketAddr::try_first_socket_addr(&addr)?,
                    handle, tls_acceptor);

                __tarpc_common_sync_listen_with! {
                    $(
                        $(#[$attr])*
                        rpc $fn_name ( $( $arg : $in_ ),* ) -> $out | $error;
                     )*
                }
            }
        }

        impl $crate::sync::Connect for SyncClient {
            fn connect<A>(addr: A, tls_context: $crate::TlsClientContext) -> ::std::result::Result<Self, ::std::io::Error>
                where A: ::std::net::ToSocketAddrs,
            {
                let addr = $crate::util::FirstSocketAddr::try_first_socket_addr(&addr)?;
                let client = <FutureClient as $crate::future::Connect>::connect(&addr, tls_context);
                let client = SyncClient($crate::futures::Future::wait(client)?);
                ::std::result::Result::Ok(client)
            }
        }

        impl<'a> $crate::future::Connect<'a> for FutureClient {
            type ConnectFut = __tarpc_service_ConnectFuture<Self>;
            type ConnectWithFut = __tarpc_service_ConnectWithFuture<'a, Self>;

            fn connect_remotely(__tarpc_service_addr: &::std::net::SocketAddr,
                                __tarpc_service_remote: &$crate::tokio_core::reactor::Remote,
                                __tarpc_tls_context: $crate::TlsClientContext)
                -> Self::ConnectFut
            {
                let client = <__tarpc_service_Client as $crate::future::Connect>::connect_remotely(
                    __tarpc_service_addr, __tarpc_service_remote, __tarpc_tls_context);

                __tarpc_service_ConnectFuture {
                    inner: $crate::futures::Future::map(client, FutureClient)
                }
            }

            fn connect_with(__tarpc_service_addr: &::std::net::SocketAddr,
                            __tarpc_service_handle: &'a $crate::tokio_core::reactor::Handle,
                            __tarpc_tls_context: $crate::TlsClientContext)
                -> Self::ConnectWithFut
            {
                let client = <__tarpc_service_Client as $crate::future::Connect>::connect_with(
                    __tarpc_service_addr, __tarpc_service_handle, __tarpc_tls_context);

                __tarpc_service_ConnectWithFuture {
                    inner: $crate::futures::Future::map(client, FutureClient)
                }
            }
        }
    }
}

// allow dead code; we're just testing that the macro expansion compiles
#[allow(dead_code)]
#[cfg(test)]
mod syntax_test {
    use util::Never;

    service! {
        #[deny(warnings)]
        #[allow(non_snake_case)]
        rpc TestCamelCaseDoesntConflict();
        rpc hello() -> String;
        #[doc="attr"]
        rpc attr(s: String) -> String;
        rpc no_args_no_return();
        rpc no_args() -> ();
        rpc one_arg(foo: String) -> i32;
        rpc two_args_no_return(bar: String, baz: u64);
        rpc two_args(bar: String, baz: u64) -> String;
        rpc no_args_ret_error() -> i32 | Never;
        rpc one_arg_ret_error(foo: String) -> String | Never;
        rpc no_arg_implicit_return_error() | Never;
        #[doc="attr"]
        rpc one_arg_implicit_return_error(foo: String) | Never;
    }
}

#[cfg(test)]
mod functional_test {
    extern crate env_logger;

    use futures::{Future, failed};
    use util::FirstSocketAddr;

    macro_rules! t {
        ($e:expr) => (match $e {
            Ok(e) => e,
            Err(e) => panic!("{} failed with {:?}", stringify!($e), e),
        })
    }

    service! {
        rpc add(x: i32, y: i32) -> i32;
        rpc hey(name: String) -> String;
    }

    cfg_if! {
        if #[cfg(feature = "tls")] {
            const DOMAIN: &'static str = "foobar.com";

            use ::TlsClientContext;
            use ::native_tls::{Pkcs12, TlsAcceptor, TlsConnector};

            fn tls_context() -> (TlsAcceptor, TlsClientContext) {
                let buf = include_bytes!("../test/identity.p12");
                let pkcs12 = t!(Pkcs12::from_der(buf, "mypass"));
                let acceptor = t!(t!(TlsAcceptor::builder(pkcs12)).build());
                let client_cx = get_tls_client_context();

                (acceptor, client_cx)
            }

            // Making the TlsConnector for testing needs to be OS-dependent just like native-tls.
            // We need to go through this trickery because the test self-signed cert is not part
            // of the system's cert chain. If it was, then all that is required is
            // `TlsConnector::builder().unwrap().build().unwrap()`.
            cfg_if! {
                if #[cfg(target_os = "macos")] {
                    extern crate security_framework;

                    use self::security_framework::certificate::SecCertificate;
                    use native_tls::backend::security_framework::TlsConnectorBuilderExt;

                    fn get_tls_client_context() -> TlsClientContext {
                        let buf = include_bytes!("../test/root-ca.der");
                        let cert = t!(SecCertificate::from_der(buf));
                        let mut connector = t!(TlsConnector::builder());
                        connector.anchor_certificates(&[cert]);

                        TlsClientContext {
                            domain: DOMAIN.into(),
                            tls_connector: t!(connector.build()),
                        }
                    }
                } else if #[cfg(all(not(target_os = "macos"), not(windows)))] {
                    use native_tls::backend::openssl::TlsConnectorBuilderExt;

                    fn get_tls_client_context() -> TlsClientContext {
                        let mut connector = t!(TlsConnector::builder());
                        t!(connector.builder_mut()
                           .builder_mut()
                           .set_ca_file("../test/root-ca.pem"));

                        TlsClientContext {
                            domain: DOMAIN.into(),
                            tls_connector: t!(connector.build()),
                        }
                    }
                // not implemented for windows or other platforms
                } else {
                    fn get_tls_client_context() -> TlsClientContext {
                        unimplemented!()
                    }
                }
            }

            fn get_sync_client<C: ::sync::Connect>(addr: &::std::net::SocketAddr) -> ::std::io::Result<C> {
                let client_cx = get_tls_client_context();
                C::connect(addr, client_cx)
            }

            fn start_server_with_sync_client<C: ::sync::Connect, S: SyncServiceExt>(server: S) -> (::std::net::SocketAddr, ::std::io::Result<C>) {
                let (tls_acceptor, client_cx) = tls_context();
                let addr = t!(server.listen("localhost:0".first_socket_addr(), tls_acceptor));
                let client = C::connect(addr, client_cx);
                (addr, client)
            }

            fn start_server_with_async_client<'a, C: ::future::Connect<'a>, S: FutureServiceExt>(server: S) -> (::std::net::SocketAddr, C) {
                let (tls_acceptor, client_cx) = tls_context();
                let addr = t!(server.listen("localhost:0".first_socket_addr(), tls_acceptor).wait());
                let client = t!(C::connect(&addr, client_cx).wait());
                (addr, client)
            }

            fn start_err_server_with_async_client<'a, C: ::future::Connect<'a>, S: error_service::FutureServiceExt>(server: S) -> (::std::net::SocketAddr, C) {
                let (tls_acceptor, client_cx) = tls_context();
                let addr = t!(server.listen("localhost:0".first_socket_addr(), tls_acceptor).wait());
                let client = t!(C::connect(&addr, client_cx).wait());
                (addr, client)
            }
        } else {
            fn get_sync_client<C: ::sync::Connect>(addr: &::std::net::SocketAddr) -> ::std::io::Result<C> {
                C::connect(addr)
            }

            /// start server and return `SyncClient`
            fn start_server_with_sync_client<C: ::sync::Connect, S: SyncServiceExt>(server: S) -> (::std::net::SocketAddr, ::std::io::Result<C>) {
                let addr = t!(server.listen("localhost:0".first_socket_addr()));
                let client = C::connect(addr);
                (addr, client)
            }

            fn start_server_with_async_client<'a, C: ::future::Connect<'a>, S: FutureServiceExt>(server: S) -> (::std::net::SocketAddr, C) {
                let addr = t!(server.listen("localhost:0".first_socket_addr()).wait());
                let client = t!(C::connect(&addr).wait());
                (addr, client)
            }

            fn start_err_server_with_async_client<'a, C: ::future::Connect<'a>, S: error_service::FutureServiceExt>(server: S) -> (::std::net::SocketAddr, C) {
                let addr = t!(server.listen("localhost:0".first_socket_addr()).wait());
                let client = t!(C::connect(&addr).wait());
                (addr, client)
            }
        }
    }

    mod sync {
        use super::{SyncClient, SyncService, env_logger, start_server_with_sync_client};
        use util::Never;

        #[derive(Clone, Copy)]
        struct Server;

        impl SyncService for Server {
            fn add(&mut self, x: i32, y: i32) -> Result<i32, Never> {
                Ok(x + y)
            }
            fn hey(&mut self, name: String) -> Result<String, Never> {
                Ok(format!("Hey, {}.", name))
            }
        }

        #[test]
        fn simple() {
            let _ = env_logger::init();
            let (_, client) = start_server_with_sync_client::<SyncClient, Server>(Server);
            let mut client = t!(client);
            assert_eq!(3, client.add(1, 2).unwrap());
            assert_eq!("Hey, Tim.", client.hey("Tim".to_string()).unwrap());
        }

        #[test]
        fn other_service() {
            let _ = env_logger::init();
            let (_, client) = start_server_with_sync_client::<super::other_service::SyncClient,
                                                              Server>(Server);
            let mut client = client.expect("Could not connect!");
            match client.foo().err().unwrap() {
                ::Error::ServerDeserialize(_) => {} // good
                bad => panic!("Expected Error::ServerDeserialize but got {}", bad),
            }
        }
    }

    mod future {
        use futures::{Finished, Future, finished};
        use super::{FutureClient, FutureService, env_logger, start_server_with_async_client};
        use util::Never;

        #[derive(Clone)]
        struct Server;

        impl FutureService for Server {
            type AddFut = Finished<i32, Never>;

            fn add(&mut self, x: i32, y: i32) -> Self::AddFut {
                finished(x + y)
            }

            type HeyFut = Finished<String, Never>;

            fn hey(&mut self, name: String) -> Self::HeyFut {
                finished(format!("Hey, {}.", name))
            }
        }

        #[test]
        fn simple() {
            let _ = env_logger::init();
            let (_, mut client) = start_server_with_async_client::<FutureClient, Server>(Server);
            assert_eq!(3, client.add(1, 2).wait().unwrap());
            assert_eq!("Hey, Tim.", client.hey("Tim".to_string()).wait().unwrap());
        }

        #[test]
        fn concurrent() {
            let _ = env_logger::init();
            let (_, mut client) = start_server_with_async_client::<FutureClient, Server>(Server);
            let req1 = client.add(1, 2);
            let req2 = client.add(3, 4);
            let req3 = client.hey("Tim".to_string());
            assert_eq!(3, req1.wait().unwrap());
            assert_eq!(7, req2.wait().unwrap());
            assert_eq!("Hey, Tim.", req3.wait().unwrap());
        }

        #[test]
        fn other_service() {
            let _ = env_logger::init();
            let (_, mut client) =
                start_server_with_async_client::<super::other_service::FutureClient,
                                                 Server>(Server);
            match client.foo().wait().err().unwrap() {
                ::Error::ServerDeserialize(_) => {} // good
                bad => panic!(r#"Expected Error::ServerDeserialize but got "{}""#, bad),
            }
        }
    }

    pub mod error_service {
        service! {
            rpc bar() -> u32 | ::util::Message;
        }
    }

    #[derive(Clone)]
    struct ErrorServer;

    impl error_service::FutureService for ErrorServer {
        type BarFut = ::futures::Failed<u32, ::util::Message>;

        fn bar(&mut self) -> Self::BarFut {
            info!("Called bar");
            failed("lol jk".into())
        }
    }

    #[test]
    fn error() {
        use std::error::Error as E;
        use self::error_service::*;
        let _ = env_logger::init();

        let (addr, mut client) = start_err_server_with_async_client::<FutureClient,
                                                                      ErrorServer>(ErrorServer);
        client.bar()
            .then(move |result| {
                match result.err().unwrap() {
                    ::Error::App(e) => {
                        assert_eq!(e.description(), "lol jk");
                        Ok::<_, ()>(())
                    } // good
                    bad => panic!("Expected Error::App but got {:?}", bad),
                }
            })
            .wait()
            .unwrap();

        let mut client = get_sync_client::<SyncClient>(&addr).unwrap();
        match client.bar().err().unwrap() {
            ::Error::App(e) => {
                assert_eq!(e.description(), "lol jk");
            } // good
            bad => panic!("Expected Error::App but got {:?}", bad),
        }
    }

    pub mod other_service {
        service! {
            rpc foo();
        }
    }
}
