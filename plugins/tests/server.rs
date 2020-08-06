#![allow(incomplete_features)]
#![feature(generic_associated_types, type_alias_impl_trait)]

use futures::Future;
use tarpc::context;

// these need to be out here rather than inside the function so that the
// assert_type_eq macro can pick them up.
#[tarpc::service]
trait Foo {
    async fn two_part(s: String, i: i32) -> (String, i32);
    async fn bar(s: String) -> String;
    async fn baz();
}

#[test]
fn type_generation_works() {
    #[tarpc::server]
    impl Foo for () {
        async fn two_part<'a>(
            &'a self,
            _: &'a mut context::Context,
            s: String,
            i: i32,
        ) -> (String, i32) {
            (s, i)
        }

        async fn bar<'a>(&'a self, _: &'a mut context::Context, s: String) -> String {
            s
        }

        async fn baz<'a>(&'a self, _: &'a mut context::Context) {}
    }

    static_assertions::assert_impl_all!(<() as Foo>::TwoPartFut<'_>: Future<Output = (String, i32)>, Send);
    static_assertions::assert_impl_all!(<() as Foo>::BarFut<'_>: Future<Output = String>, Send);
    static_assertions::assert_impl_all!(<() as Foo>::BazFut<'_>: Future<Output = ()>, Send);
}

#[allow(non_camel_case_types)]
#[test]
fn raw_idents_work() {
    type r#yield = String;

    #[tarpc::service]
    trait r#trait {
        async fn r#await(r#struct: r#yield, r#enum: i32) -> (r#yield, i32);
        async fn r#fn(r#impl: r#yield) -> r#yield;
        async fn r#async();
    }

    #[tarpc::server]
    impl r#trait for () {
        async fn r#await<'a>(
            &'a self,
            _: &'a mut context::Context,
            r#struct: r#yield,
            r#enum: i32,
        ) -> (r#yield, i32) {
            (r#struct, r#enum)
        }

        async fn r#fn<'a>(&'a self, _: &'a mut context::Context, r#impl: r#yield) -> r#yield {
            r#impl
        }

        async fn r#async<'a>(&'a self, _: &'a mut context::Context) {}
    }
}

#[test]
fn syntax() {
    #[tarpc::service]
    trait Syntax {
        async fn no_lifetimes();
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

    #[tarpc::server]
    impl Syntax for () {
        async fn no_lifetimes(&self, _: &mut context::Context) {}

        #[deny(warnings)]
        #[allow(non_snake_case)]
        async fn TestCamelCaseDoesntConflict<'a>(&self, _: &'a mut context::Context) {}

        async fn hello<'a>(&self, _: &'a mut context::Context) -> String {
            String::new()
        }

        async fn attr<'a>(&self, _: &'a mut context::Context, _s: String) -> String {
            String::new()
        }

        async fn no_args_no_return<'a>(&self, _: &'a mut context::Context) {}

        async fn no_args<'a>(&self, _: &'a mut context::Context) -> () {}

        async fn one_arg<'a>(&self, _: &'a mut context::Context, _one: String) -> i32 {
            0
        }

        async fn two_args_no_return<'a>(
            &'a self,
            _: &'a mut context::Context,
            _one: String,
            _two: u64,
        ) {
        }

        async fn two_args<'a>(
            &'a self,
            _: &'a mut context::Context,
            _one: String,
            _two: u64,
        ) -> String {
            String::new()
        }

        async fn no_args_ret_error<'a>(&'a self, _: &'a mut context::Context) -> i32 {
            0
        }

        async fn one_arg_ret_error<'a>(
            &'a self,
            _: &'a mut context::Context,
            _one: String,
        ) -> String {
            String::new()
        }

        async fn no_arg_implicit_return_error<'a>(&'a self, _: &'a mut context::Context) {}

        async fn one_arg_implicit_return_error<'a>(
            &'a self,
            _: &'a mut context::Context,
            _one: String,
        ) {
        }
    }
}
