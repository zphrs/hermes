extern crate proc_macro;
#[proc_macro]
pub fn assert_sizes_match<A, B>(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    if core::mem::size_of::<A>() == core::mem::size_of::<B>() {
        let out = proc_macro::TokenStream::new();
        out
    } else {
        panic!("invalid")
    }
}
