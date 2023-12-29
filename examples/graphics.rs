#![feature(allocator_api)]

use stupidalloc::StupidAlloc;
#[global_allocator]
static GLOBAL: StupidAlloc = StupidAlloc;

#[cfg(feature = "always-graphics")]
fn main() {
    use std::io::Read;

    let mut string = String::with_capacity(16);

    println!("Type what you want below, it will be echoed back to you!");

    std::io::stdin().lock().read_to_string(&mut string).unwrap();

    println!("{string}");
}

#[cfg(not(feature = "always-graphics"))]
fn main() {
    eprintln!("This example is made to showcase the graphical display of stupidalloc. Running it without the `always-graphics` feature is useless.");
}
