#![feature(allocator_api)]

use stupidalloc::StupidAlloc;
#[global_allocator]
static GLOBAL: StupidAlloc = StupidAlloc;

#[cfg(feature = "always-graphics")]
fn main() {
    use std::io::Read;

    let mut string = String::with_capacity(16);

    std::io::stdin().lock().read_to_string(&mut string).unwrap();

    println!("{string}");
}

#[cfg(not(feature = "always-graphics"))]
fn main() {
    eprintln!("This example is made to showcase the graphical display of stupidalloc. Running it without the `always-graphical` feature is useless.");
}
