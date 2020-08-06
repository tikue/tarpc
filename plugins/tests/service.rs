#![allow(incomplete_features)]
#![feature(generic_associated_types, type_alias_impl_trait)]

use tarpc::context;

#[test]
fn att_service_trait() {
    #[tarpc::service]
    trait Foo {
        async fn two_part(s: String, i: i32) -> (String, i32);
        async fn bar(s: String) -> String;
        async fn baz();
    }

    #[tarpc::server]
    impl Foo for () {
        async fn two_part(&self, _: &mut context::Context, s: String, i: i32) -> (String, i32) {
            (s, i)
        }

        async fn bar(&self, _: &mut context::Context, s: String) -> String {
            s
        }

        async fn baz(&self, _: &mut context::Context) {}
    }
}

#[allow(non_camel_case_types)]
#[test]
fn raw_idents() {
    use futures::future::{ready, Ready};

    type r#yield = String;

    #[tarpc::service]
    trait r#trait {
        async fn r#await(r#struct: r#yield, r#enum: i32) -> (r#yield, i32);
        async fn r#fn(r#impl: r#yield) -> r#yield;
        async fn r#async();
    }

    impl r#trait for () {
        type AwaitFut<'a> = Ready<(r#yield, i32)>;
        fn r#await<'a>(
            &'a self,
            _: &'a mut context::Context,
            r#struct: r#yield,
            r#enum: i32,
        ) -> Self::AwaitFut<'a> {
            ready((r#struct, r#enum))
        }

        type FnFut<'a> = Ready<r#yield>;
        fn r#fn<'a>(&'a self, _: &'a mut context::Context, r#impl: r#yield) -> Self::FnFut<'a> {
            ready(r#impl)
        }

        type AsyncFut<'a> = Ready<()>;
        fn r#async<'a>(&'a self, _: &'a mut context::Context) -> Self::AsyncFut<'a> {
            ready(())
        }
    }
}

#[test]
fn syntax() {
    #[tarpc::service]
    trait Syntax {
        #[deny(warnings)]
        #[allow(non_snake_case)]
        async fn TestCamelCaseDoesntConflict();
        async fn hello() -> String;
        #[doc = "attr"]
        async fn attr(s: String) -> String;
        async fn no_args_no_return();
        async fn no_args() -> ();
        async fn one_arg(one: String) -> i32;
        async fn two_args_no_return(one: String, two: u64);
        async fn two_args(one: String, two: u64) -> String;
        async fn no_args_ret_error() -> i32;
        async fn one_arg_ret_error(one: String) -> String;
        async fn no_arg_implicit_return_error();
        #[doc = "attr"]
        async fn one_arg_implicit_return_error(one: String);
    }
}
